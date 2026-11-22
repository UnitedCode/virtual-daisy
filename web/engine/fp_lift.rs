//! Lift short pure-VFP sequences into register-local Rust expressions.
//! Values commit after all speculative operations pass their IEEE guards.
use std::collections::{BTreeMap, BTreeSet};
use zmu_cortex_m::core::{
    instruction::{AddressingMode, Imm32Carry, Instruction, SRType, SetFlags, VMRSTarget},
    register::{ExtensionReg, Reg},
};
struct Emitter {
    code: String,
    regs: BTreeMap<usize, String>,
    dirty: BTreeSet<usize>,
    stores: Vec<(String, String)>,
    next: usize,
    stored: bool,
    gprs: BTreeMap<usize, String>,
    gpr_dirty: BTreeSet<usize>,
    status_dirty: bool,
    cycles: u32,
    branch: Option<(u32, String, u32)>,
    predicate: Option<String>,
    initial_fp: BTreeSet<usize>,
    fp_status_dirty: bool,
    uses_fp: bool,
}
impl Emitter {
    fn sets_flags(&self, flags: SetFlags) -> bool {
        flags == SetFlags::True || flags == SetFlags::NotInITBlock && self.predicate.is_none()
    }
    fn status(&mut self, code: String) {
        self.status_dirty = true;
        if let Some(predicate) = &self.predicate {
            self.code.push_str(&format!("if {predicate}{{{code}}}\n"));
        } else {
            self.code.push_str(&code);
        }
    }
    fn logic_flags(&mut self, value: &str, carry: &str) {
        self.status(format!("_status=(_status&0x1fff_ffff)|({value}&0x8000_0000)|((({value}==0)as u32)<<30)|(({carry} as u32)<<29);\n"));
    }
    fn immediate(&mut self, immediate: Imm32Carry) -> (String, String) {
        let (value, carry) = (self.name(), self.name());
        let expression = match immediate {
            Imm32Carry::NoCarry { imm32 } => format!("({imm32}u32,_status&(1<<29)!=0)"),
            Imm32Carry::Carry { imm32_c0, imm32_c1 } => {
                format!("if _status&(1<<29)!=0{{{imm32_c1:?}}}else{{{imm32_c0:?}}}")
            }
        };
        self.code
            .push_str(&format!("let ({value},{carry}):(u32,bool)={expression};\n"));
        (value, carry)
    }
    fn shift(&mut self, value: &str, kind: SRType, amount: u8) -> (String, String) {
        let (shifted, carry) = (self.name(), self.name());
        self.code.push_str(&format!("let ({shifted},{carry})=zmu_cortex_m::core::operation::shift_c({value},SRType::{kind:?},{amount},_status&(1<<29)!=0);\n"));
        (shifted, carry)
    }
    fn word_load(&mut self, address: &str, dest: Reg) -> Option<()> {
        if self.stored || self.predicate.is_some() {
            return None;
        }
        let value = self.name();
        self.code.push_str(&format!(
            "let Some({value})=cpu.fp_lift_load({address})else{{return false;}};\n"
        ));
        self.gpr_dest(dest, value)?;
        self.cycles += 1;
        Some(())
    }
    fn word_store(&mut self, address: String, value: String) {
        self.code.push_str(&format!(
            "if !cpu.fp_lift_store_address({address}){{return false;}}\n"
        ));
        self.stores.push((address, value));
        self.stored = true;
        self.cycles += 1;
    }
    fn gpr(&mut self, reg: Reg) -> Option<String> {
        if reg == Reg::PC {
            return None;
        }
        let index = reg as usize;
        if let Some(value) = self.gprs.get(&index) {
            return Some(value.clone());
        }
        let name = self.name();
        self.code
            .push_str(&format!("let {name}=cpu.get_r(Reg::{reg:?});\n"));
        self.gprs.insert(index, name.clone());
        Some(name)
    }
    fn gpr_dest(&mut self, reg: Reg, value: String) -> Option<()> {
        if matches!(reg, Reg::SP | Reg::PC) {
            return None;
        }
        let value = if let Some(predicate) = self.predicate.clone() {
            let old = self.gpr(reg)?;
            let selected = self.name();
            self.code.push_str(&format!(
                "let {selected}=if {predicate}{{{value}}}else{{{old}}};\n"
            ));
            selected
        } else {
            value
        };
        self.gprs.insert(reg as usize, value);
        self.gpr_dirty.insert(reg as usize);
        Some(())
    }
    fn integer_math(&mut self, a: &str, b: &str, subtract: bool, flags: bool) -> String {
        let v = self.name();
        let op = if subtract {
            "wrapping_sub"
        } else {
            "wrapping_add"
        };
        self.code.push_str(&format!("let {v}={a}.{op}({b});\n"));
        if flags {
            let carry = if subtract {
                format!("({a}>={b})as u32")
            } else {
                format!("((u64::from({a})+u64::from({b}))>>32)as u32")
            };
            let overflow = if subtract {
                format!("(({a}^{b})&({a}^{v}))>>31")
            } else {
                format!("(!({a}^{b})&({a}^{v}))>>31")
            };
            self.status(format!("_status=(_status&0x0fff_ffff)|({v}&0x8000_0000)|((({v}==0)as u32)<<30)|(({carry})<<29)|(({overflow})<<28);\n"));
        }
        v
    }
    fn carry_math(&mut self, a: &str, b: &str, subtract: bool, flags: bool) -> String {
        let (value, carry, overflow) = (self.name(), self.name(), self.name());
        let right = if subtract { format!("!{b}") } else { b.into() };
        self.code.push_str(&format!("let ({value},{carry},{overflow})=zmu_cortex_m::core::operation::add_with_carry({a},{right},_status&(1<<29)!=0);\n"));
        if flags {
            self.status(format!("_status=(_status&0x0fff_ffff)|({value}&0x8000_0000)|((({value}==0)as u32)<<30)|(({carry} as u32)<<29)|(({overflow} as u32)<<28);\n"));
        }
        value
    }
    fn name(&mut self) -> String {
        let name = format!("v{}", self.next);
        self.next += 1;
        name
    }
    fn input(&mut self, index: usize) -> String {
        if let Some(v) = self.regs.get(&index) {
            return v.clone();
        }
        let v = self.name();
        self.code
            .push_str(&format!("let {v}=f32::from_bits(cpu.fp_regs[{index}]);\n"));
        self.initial_fp.insert(index);
        self.regs.insert(index, v.clone());
        v
    }
    fn dest(&mut self, index: usize, value: String) {
        let value = if let Some(predicate) = self.predicate.clone() {
            let old = self.input(index);
            let selected = self.name();
            self.code.push_str(&format!(
                "let {selected}=if {predicate}{{{value}}}else{{{old}}};\n"
            ));
            selected
        } else {
            value
        };
        self.regs.insert(index, value);
        self.dirty.insert(index);
    }
    fn math(&mut self, op: u8, a: &str, b: &str) -> String {
        let v = self.name();
        let operator = ["+", "-", "*", "/"][op as usize];
        self.code.push_str(&format!("let {v}={a}{operator}{b};\n"));
        let zero_function = [
            "lift_zero_add",
            "lift_zero_sub",
            "lift_zero_mul",
            "lift_zero_div",
        ][op as usize];
        self.code.push_str(&format!("let b{v}={v}.to_bits()&0x7fff_ffff;valid&=(_sticky&&b{v}.wrapping_sub(0x0080_0001)<0x7eff_ffff)||{zero_function}({v},{a},{b});\n"));
        v
    }
    fn memory_address(&mut self, rn: Reg, add: bool, offset: u32, pc: u32) -> String {
        let a = self.name();
        let base = if rn == Reg::PC {
            format!("{}u32", (pc + 4) & !3)
        } else {
            self.gpr(rn).unwrap()
        };
        let operation = if add { "wrapping_add" } else { "wrapping_sub" };
        self.code
            .push_str(&format!("let {a}={base}.{operation}({offset});\n"));
        a
    }
}
pub fn emit(instructions: &[(Instruction, usize)], pc: u32) -> Option<String> {
    let mut e = Emitter {
        code: String::from(
            "let _sticky=cpu.fpscr&16!=0;let mut valid=true;#[allow(unused_mut)]let mut _status=cpu.psr.value;#[allow(unused_mut)]let mut _fpscr=cpu.fpscr;\n",
        ),
        regs: BTreeMap::new(),
        dirty: BTreeSet::new(),
        stores: Vec::new(),
        next: 0,
        stored: false,
        gprs:BTreeMap::new(),gpr_dirty:BTreeSet::new(),status_dirty:false,cycles:0,branch:None,predicate:None,initial_fp:BTreeSet::new(),fp_status_dirty:false,uses_fp:false,
    };
    let mut at = pc;
    for (position, (i, size)) in instructions.iter().enumerate() {
        let predicated = e.predicate.is_some();
        if e.branch.is_some() {
            return None;
        }
        e.cycles += 1;
        match i {
            Instruction::IT {
                firstcond, mask: 8, ..
            } if !predicated => {
                // Match the interpreter's four-cycle IT retirement exactly.
                e.cycles += 3;
                let condition = e.name();
                e.code.push_str(&format!("let {condition}=zmu_cortex_m::core::operation::condition_test(zmu_cortex_m::core::condition::Condition::{firstcond:?},&zmu_cortex_m::core::register::PSR{{value:_status}});\n"));
                e.predicate = Some(condition);
                at += *size as u32;
                continue;
            }
            Instruction::B_t13 { params, .. } => {
                if predicated {
                    return None;
                }
                let condition = e.name();
                e.code.push_str(&format!("let {condition}=zmu_cortex_m::core::operation::condition_test(zmu_cortex_m::core::condition::Condition::{:?},&zmu_cortex_m::core::register::PSR{{value:_status}});\n",params.cond));
                if position + 1 < instructions.len() {
                    e.code
                        .push_str(&format!("if {condition}{{return false;}}\n"));
                } else {
                    e.branch = Some((
                        at.wrapping_add(4).wrapping_add_signed(params.imm32),
                        condition,
                        2,
                    ));
                }
            }
            Instruction::B_t24 { imm32, .. } => {
                if predicated {
                    return None;
                }
                e.branch = Some((
                    at.wrapping_add(4).wrapping_add_signed(*imm32),
                    "true".into(),
                    2,
                ));
            }
            Instruction::CBZ { params } | Instruction::CBNZ { params } => {
                if predicated {
                    return None;
                }
                let value = e.gpr(params.rn)?;
                let condition = e.name();
                let operator = if matches!(i, Instruction::CBZ { .. }) {
                    "=="
                } else {
                    "!="
                };
                e.code
                    .push_str(&format!("let {condition}={value}{operator}0;\n"));
                if position + 1 < instructions.len() {
                    e.code
                        .push_str(&format!("if {condition}{{return false;}}\n"));
                } else {
                    e.branch = Some((at.wrapping_add(4).wrapping_add(params.imm32), condition, 0));
                }
            }
            Instruction::MOV_imm { params, .. } => {
                let (value, carry) = e.immediate(params.imm32);
                if e.sets_flags(params.setflags) {
                    e.logic_flags(&value, &carry);
                }
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::MOV_reg { params, .. } => {
                let value = e.gpr(params.rm)?;
                if e.sets_flags(params.setflags) {
                    e.logic_flags(&value, "(_status&(1<<29)!=0)");
                }
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::MOVT { params } => {
                let old = e.gpr(params.rd)?;
                let value = e.name();
                e.code.push_str(&format!(
                    "let {value}=({old}&0xffff)|({}u32<<16);\n",
                    params.imm16
                ));
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::CLZ { params } => {
                let source = e.gpr(params.rm)?;
                let value = e.name();
                e.code
                    .push_str(&format!("let {value}={source}.leading_zeros();\n"));
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::UBFX { params } => {
                if params.lsb + params.widthminus1 > 31 {
                    return None;
                }
                let source = e.gpr(params.rn)?;
                let mask = ((1u64 << (params.widthminus1 + 1)) - 1) as u32;
                let value = e.name();
                e.code
                    .push_str(&format!("let {value}=({source}>>{})&{mask};\n", params.lsb));
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::BFC { params } => {
                if params.msbit < params.lsbit || params.msbit >= 32 {
                    return None;
                }
                let old = e.gpr(params.rd)?;
                let mask =
                    (((1u64 << (params.msbit - params.lsbit + 1)) - 1) << params.lsbit) as u32;
                let value = e.name();
                e.code.push_str(&format!("let {value}={old}&!{mask}u32;\n"));
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::BFI { params } => {
                if params.width == 0 || params.lsbit + params.width > 32 {
                    return None;
                }
                let old = e.gpr(params.rd)?;
                let source = e.gpr(params.rn)?;
                let mask = (((1u64 << params.width) - 1) << params.lsbit) as u32;
                let value = e.name();
                e.code.push_str(&format!(
                    "let {value}=({old}&!{mask}u32)|(({source}<<{})&{mask});\n",
                    params.lsbit
                ));
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::UMULL { params } | Instruction::UMLAL { params } => {
                let a = e.gpr(params.rn)?;
                let b = e.gpr(params.rm)?;
                let value = e.name();
                let accumulate = if matches!(i, Instruction::UMLAL { .. }) {
                    let low = e.gpr(params.rdlo)?;
                    let high = e.gpr(params.rdhi)?;
                    format!(".wrapping_add((u64::from({high})<<32)|u64::from({low}))")
                } else {
                    String::new()
                };
                e.code.push_str(&format!(
                    "let {value}=u64::from({a}).wrapping_mul(u64::from({b})){accumulate};\n"
                ));
                let (low, high) = (e.name(), e.name());
                e.code.push_str(&format!(
                    "let {low}={value} as u32;let {high}=({value}>>32) as u32;\n"
                ));
                e.gpr_dest(params.rdlo, low)?;
                e.gpr_dest(params.rdhi, high)?;
            }
            Instruction::AND_imm { params }
            | Instruction::BIC_imm { params }
            | Instruction::ORR_imm { params }
            | Instruction::EOR_imm { params } => {
                let left = e.gpr(params.rn)?;
                let (right, carry) = e.immediate(params.imm32);
                let operator = match i {
                    Instruction::AND_imm { .. } => "&",
                    Instruction::BIC_imm { .. } => "&!",
                    Instruction::ORR_imm { .. } => "|",
                    _ => "^",
                };
                let value = e.name();
                e.code
                    .push_str(&format!("let {value}={left}{operator}{right};\n"));
                if params.setflags {
                    e.logic_flags(&value, &carry);
                }
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::AND_reg { params, .. }
            | Instruction::BIC_reg { params, .. }
            | Instruction::ORR_reg { params, .. }
            | Instruction::EOR_reg { params, .. } => {
                let left = e.gpr(params.rn)?;
                let rm = e.gpr(params.rm)?;
                let (right, carry) = e.shift(&rm, params.shift_t, params.shift_n);
                let operator = match i {
                    Instruction::AND_reg { .. } => "&",
                    Instruction::BIC_reg { .. } => "&!",
                    Instruction::ORR_reg { .. } => "|",
                    _ => "^",
                };
                let value = e.name();
                e.code
                    .push_str(&format!("let {value}={left}{operator}{right};\n"));
                if e.sets_flags(params.setflags) {
                    e.logic_flags(&value, &carry);
                }
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::LSL_imm { params, .. }
            | Instruction::LSR_imm { params, .. }
            | Instruction::ASR_imm { params, .. } => {
                let source = e.gpr(params.rm)?;
                let kind = match i {
                    Instruction::LSL_imm { .. } => SRType::LSL,
                    Instruction::LSR_imm { .. } => SRType::LSR,
                    _ => SRType::ASR,
                };
                let (value, carry) = e.shift(&source, kind, params.shift_n);
                if e.sets_flags(params.setflags) {
                    e.logic_flags(&value, &carry);
                }
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::LSL_reg { params, .. }
            | Instruction::LSR_reg { params, .. }
            | Instruction::ASR_reg { params, .. } => {
                let source = e.gpr(params.rn)?;
                let amount = e.gpr(params.rm)?;
                let (value, carry) = (e.name(), e.name());
                let kind = match i {
                    Instruction::LSL_reg { .. } => "LSL",
                    Instruction::LSR_reg { .. } => "LSR",
                    _ => "ASR",
                };
                e.code.push_str(&format!("let ({value},{carry})=zmu_cortex_m::core::operation::shift_c({source},SRType::{kind},({amount}&255) as usize,_status&(1<<29)!=0);\n"));
                if e.sets_flags(params.setflags) {
                    e.logic_flags(&value, &carry);
                }
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::LDRB_imm { params, .. } => {
                if e.stored || predicated {
                    return None;
                }
                let base = e.gpr(params.rn)?;
                let offset = e.name();
                let op = if params.add {
                    "wrapping_add"
                } else {
                    "wrapping_sub"
                };
                e.code
                    .push_str(&format!("let {offset}={base}.{op}({});\n", params.imm32));
                let address = if params.index { offset.clone() } else { base };
                let word = e.name();
                let value = e.name();
                e.code.push_str(&format!("let Some({word})=cpu.fp_lift_load({address}&!3)else{{return false;}};let {value}=({word}>>(({address}&3)*8))&255;\n"));
                if params.wback {
                    e.gpr_dest(params.rn, offset)?;
                }
                e.gpr_dest(params.rt, value)?;
                e.cycles += 1;
            }
            Instruction::LDRD_imm { params } | Instruction::STRD_imm { params } => {
                if predicated {
                    return None;
                }
                let base = e.gpr(params.rn)?;
                let offset = e.name();
                let op = if params.add {
                    "wrapping_add"
                } else {
                    "wrapping_sub"
                };
                e.code
                    .push_str(&format!("let {offset}={base}.{op}({});\n", params.imm32));
                let address = if params.index { offset.clone() } else { base };
                e.code
                    .push_str(&format!("if {address}&3!=0{{return false;}}\n"));
                if matches!(i, Instruction::LDRD_imm { .. }) {
                    e.word_load(&address, params.rt)?;
                    e.word_load(&format!("{address}.wrapping_add(4)"), params.rt2)?;
                } else {
                    let first = e.gpr(params.rt)?;
                    let second = e.gpr(params.rt2)?;
                    e.word_store(address.clone(), first);
                    e.word_store(format!("{address}.wrapping_add(4)"), second);
                }
                e.cycles -= 1;
                if params.wback {
                    e.gpr_dest(params.rn, offset)?;
                }
            }
            Instruction::LDR_imm { params, .. } | Instruction::STR_imm { params, .. } => {
                if predicated {
                    return None;
                }
                let base = e.gpr(params.rn)?;
                let offset = e.name();
                let operation = if params.add {
                    "wrapping_add"
                } else {
                    "wrapping_sub"
                };
                e.code.push_str(&format!(
                    "let {offset}={base}.{operation}({});\n",
                    params.imm32
                ));
                let address = if params.index { offset.clone() } else { base };
                if matches!(i, Instruction::LDR_imm { .. }) {
                    if params.wback {
                        e.gpr_dest(params.rn, offset)?;
                    }
                    e.word_load(&address, params.rt)?;
                } else {
                    let value = e.gpr(params.rt)?;
                    if params.wback {
                        e.gpr_dest(params.rn, offset)?;
                    }
                    e.word_store(address, value);
                }
            }
            Instruction::LDR_reg { params, .. } | Instruction::STR_reg { params, .. } => {
                if predicated {
                    return None;
                }
                if matches!(i, Instruction::STR_reg { .. })
                    && (!params.add || !params.index || params.wback)
                {
                    return None;
                }
                let base = e.gpr(params.rn)?;
                let rm = e.gpr(params.rm)?;
                let (shifted, _) = e.shift(&rm, params.shift_t, params.shift_n);
                let offset = e.name();
                let operation = if params.add {
                    "wrapping_add"
                } else {
                    "wrapping_sub"
                };
                e.code
                    .push_str(&format!("let {offset}={base}.{operation}({shifted});\n"));
                let address = if params.index { offset.clone() } else { base };
                if matches!(i, Instruction::LDR_reg { .. }) {
                    if params.wback {
                        e.gpr_dest(params.rn, offset)?;
                    }
                    e.word_load(&address, params.rt)?;
                } else {
                    let value = e.gpr(params.rt)?;
                    e.word_store(address, value);
                }
            }
            Instruction::VLDM_T2 { params } => {
                if e.stored || predicated {
                    return None;
                }
                let base = e.gpr(params.rn)?;
                let offset = if params.mode == AddressingMode::IncrementAfter {
                    0
                } else {
                    params.imm32
                };
                let address = e.name();
                e.code
                    .push_str(&format!("let {address}={base}.wrapping_sub({offset});\n"));
                for (n, reg) in (&params.list).into_iter().enumerate() {
                    let value = e.name();
                    e.code.push_str(&format!("let Some({value})=cpu.fp_lift_load({address}.wrapping_add({}))else{{return false;}};let {value}=f32::from_bits({value});valid&=lift_ordinary({value});\n",n*4));
                    e.dest(reg as usize, value);
                }
                if params.write_back {
                    let value = e.name();
                    let operation = if params.mode == AddressingMode::IncrementAfter {
                        "wrapping_add"
                    } else {
                        "wrapping_sub"
                    };
                    e.code.push_str(&format!(
                        "let {value}={base}.{operation}({});\n",
                        params.imm32
                    ));
                    e.gpr_dest(params.rn, value)?;
                }
            }
            Instruction::ADD_imm { params, .. }
            | Instruction::SUB_imm { params, .. }
            | Instruction::RSB_imm { params, .. } => {
                let a = e.gpr(params.rn)?;
                let b = format!("{}u32", params.imm32);
                let (a, b) = if matches!(i, Instruction::RSB_imm { .. }) {
                    (b, a)
                } else {
                    (a, b)
                };
                let v = e.integer_math(
                    &a,
                    &b,
                    !matches!(i, Instruction::ADD_imm { .. }),
                    e.sets_flags(params.setflags),
                );
                e.gpr_dest(params.rd, v)?;
            }
            Instruction::ADC_imm { params } | Instruction::SBC_imm { params } => {
                let a = e.gpr(params.rn)?;
                let b = format!("{}u32", params.imm32);
                let value = e.carry_math(
                    &a,
                    &b,
                    matches!(i, Instruction::SBC_imm { .. }),
                    e.sets_flags(params.setflags),
                );
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::ADC_reg { params, .. } | Instruction::SBC_reg { params, .. } => {
                let a = e.gpr(params.rn)?;
                let b = e.gpr(params.rm)?;
                let (b, _) = e.shift(&b, params.shift_t, params.shift_n);
                let value = e.carry_math(
                    &a,
                    &b,
                    matches!(i, Instruction::SBC_reg { .. }),
                    e.sets_flags(params.setflags),
                );
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::ADD_reg { params, .. }
            | Instruction::SUB_reg { params, .. }
            | Instruction::RSB_reg { params, .. } => {
                let a = e.gpr(params.rn)?;
                let rm = e.gpr(params.rm)?;
                let (shifted, _) = e.shift(&rm, params.shift_t, params.shift_n);
                let (a, b) = if matches!(i, Instruction::RSB_reg { .. }) {
                    (shifted, a)
                } else {
                    (a, shifted)
                };
                let v = e.integer_math(
                    &a,
                    &b,
                    !matches!(i, Instruction::ADD_reg { .. }),
                    e.sets_flags(params.setflags),
                );
                e.gpr_dest(params.rd, v)?;
            }
            Instruction::CMP_imm { params, .. } => {
                let a = e.gpr(params.r)?;
                let b = format!("{}u32", params.imm32);
                e.integer_math(&a, &b, true, true);
            }
            Instruction::CMP_reg { params, .. } => {
                let a = e.gpr(params.rn)?;
                let rm = e.gpr(params.rm)?;
                let (shifted, _) = e.shift(&rm, params.shift_t, params.shift_n);
                e.integer_math(&a, &shifted, true, true);
            }
            Instruction::VLDR { params } => {
                if e.stored || predicated {
                    return None;
                }
                let ExtensionReg::Single { reg } = params.dd else {
                    return None;
                };
                let address = e.memory_address(params.rn, params.add, params.imm32, at);
                let v = e.name();
                e.code.push_str(&format!("let Some({v})=cpu.fp_lift_load({address})else{{return false;}};let {v}=f32::from_bits({v});valid&=lift_ordinary({v});\n"));
                e.dest(reg as usize, v);
            }
            Instruction::VSTR { params } => {
                if predicated {
                    return None;
                }
                let ExtensionReg::Single { reg } = params.dd else {
                    return None;
                };
                let address = e.memory_address(params.rn, params.add, params.imm32, at);
                let value = e.input(reg as usize);
                e.code.push_str(&format!(
                    "if !cpu.fp_lift_store_address({address}){{return false;}}\n"
                ));
                e.stores.push((address, format!("{value}.to_bits()")));
                e.stored = true;
            }
            Instruction::VSQRT_f32 { params } => {
                let source = e.input(params.sm as usize);
                let value = e.name();
                e.code.push_str(&format!("let {value}={source}.sqrt();valid&={source}>=0.&&lift_ordinary({value})&&(_sticky||f64::from({value})*f64::from({value})==f64::from({source}));\n"));
                e.dest(params.sd as usize, value);
            }
            Instruction::VADD_f32 { params }
            | Instruction::VSUB_f32 { params }
            | Instruction::VMUL_f32 { params }
            | Instruction::VDIV_f32 { params } => {
                let a = e.input(params.sn as usize);
                let b = e.input(params.sm as usize);
                let op = match i {
                    Instruction::VADD_f32 { .. } => 0,
                    Instruction::VSUB_f32 { .. } => 1,
                    Instruction::VMUL_f32 { .. } => 2,
                    _ => 3,
                };
                let v = e.math(op, &a, &b);
                e.dest(params.sd as usize, v);
            }
            Instruction::VMLA_f32 {
                params,
                subtract,
                negate,
            } => {
                let a = e.input(params.sn as usize);
                let b = e.input(params.sm as usize);
                let d = e.input(params.sd as usize);
                let product = e.math(2, &a, &b);
                let d = if *negate { format!("-{d}") } else { d };
                let product = if *subtract {
                    format!("-{product}")
                } else {
                    product
                };
                let v = e.math(0, &d, &product);
                e.dest(params.sd as usize, v);
            }
            Instruction::VCMP_f32 { params } => {
                let a = e.input(params.sd as usize);
                let b = if params.with_zero {
                    "0f32".into()
                } else {
                    e.input(params.sm as usize)
                };
                let update = format!("_fpscr=(_fpscr&0x0fff_ffff)|if {a}=={b}{{0x6000_0000}}else if {a}<{b}{{0x8000_0000}}else{{0x2000_0000}};\n");
                if let Some(predicate) = &e.predicate {
                    e.code.push_str(&format!("if {predicate}{{{update}}}\n"));
                } else {
                    e.code.push_str(&update);
                }
                e.fp_status_dirty = true;
            }
            Instruction::VMRS { rt } => {
                e.uses_fp = true;
                match rt {
                    VMRSTarget::APSRNZCV => {
                        e.status("_status=(_status&0x0fff_ffff)|(_fpscr&0xf000_0000);\n".into())
                    }
                    VMRSTarget::Register(reg) => {
                        let value = e.name();
                        e.code.push_str(&format!("let {value}=_fpscr;\n"));
                        e.gpr_dest(*reg, value)?;
                    }
                }
            }
            Instruction::VSEL_f32 { params } if !predicated => {
                let a = e.input(params.sn as usize);
                let b = e.input(params.sm as usize);
                let value = e.name();
                e.code.push_str(&format!("let {value}=if zmu_cortex_m::core::operation::condition_test(zmu_cortex_m::core::condition::Condition::{:?},&zmu_cortex_m::core::register::PSR{{value:_status}}){{{a}}}else{{{b}}};\n", params.cond));
                e.dest(params.sd as usize, value);
            }
            Instruction::VMOV_reg_f32 { params } => {
                let v = e.input(params.sm as usize);
                e.dest(params.sd as usize, v);
            }
            Instruction::VMOV_imm_32 { params } => {
                let v = e.name();
                e.code.push_str(&format!(
                    "let {v}=f32::from_bits({});valid&=lift_ordinary({v});\n",
                    params.imm32
                ));
                e.dest(params.sd as usize, v);
            }
            Instruction::VABS_f32 { params } | Instruction::VNEG_f32 { params } => {
                let source = e.input(params.sm as usize);
                let v = e.name();
                let expression = if matches!(i, Instruction::VABS_f32 { .. }) {
                    format!("{source}.abs()")
                } else {
                    format!("-{source}")
                };
                e.code.push_str(&format!("let {v}={expression};\n"));
                e.dest(params.sd as usize, v);
            }
            _ => return None,
        }
        at += *size as u32;
        if predicated {
            e.predicate = None;
        }
    }
    if e.predicate.is_some() {
        return None;
    }
    e.code.push_str("if valid{\n");
    // In native unit tests each accepted block is shadowed by ordinary execution,
    // including all FP bits, flags, changed memory, PC and retired instruction count.
    e.code.push_str("#[cfg(test)] let expected={let old_fp=cpu.fp_regs;let old_status=cpu.fpscr;let old_pc=cpu.get_pc();let old_count=cpu.instruction_count;let old_gprs=cpu.r0_12;let old_lr=cpu.lr;let old_psr=cpu.psr.value;let mut expected_cycles=0u32;\n");
    for (n, (address, _)) in e.stores.iter().enumerate() {
        e.code.push_str(&format!(
            "let old{n}=cpu.fp_lift_load({address}).unwrap();\n"
        ));
    }
    for (i, size) in instructions {
        let expression = super::instruction_expr(i);
        e.code.push_str(&format!(
            "expected_cycles+=zmu_cortex_m::executor::Executor::execute(cpu,&{expression},{size});\n"
        ));
    }
    e.code
        .push_str("let result:([u32;32],u32,u32,u64,Vec<u32>,[u32;13],u32,u32,u32)=(cpu.fp_regs,cpu.fpscr,cpu.get_pc(),cpu.instruction_count,vec![");
    for (address, _) in &e.stores {
        e.code
            .push_str(&format!("cpu.fp_lift_load({address}).unwrap(),"));
    }
    e.code.push_str("],cpu.r0_12,cpu.psr.value,expected_cycles,cpu.lr);cpu.fp_regs=old_fp;cpu.fpscr=old_status;cpu.set_pc(old_pc);cpu.instruction_count=old_count;cpu.r0_12=old_gprs;cpu.lr=old_lr;cpu.psr.value=old_psr;\n");
    for (n, (address, _)) in e.stores.iter().enumerate() {
        e.code.push_str(&format!(
            "cpu.fp_lift_store_word({address},old{n}).unwrap();\n"
        ));
    }
    e.code.push_str("result};\n");
    for index in &e.gpr_dirty {
        let value = &e.gprs[index];
        if *index == 14 {
            e.code.push_str(&format!("cpu.lr={value};\n"));
        } else {
            e.code.push_str(&format!("cpu.r0_12[{index}]={value};\n"));
        }
    }
    if e.status_dirty {
        e.code.push_str("cpu.psr.value=_status;\n");
    }
    if e.fp_status_dirty {
        e.code.push_str("cpu.fpscr=_fpscr;\n");
    }
    for index in &e.dirty {
        let v = &e.regs[index];
        e.code
            .push_str(&format!("cpu.fp_regs[{index}]={v}.to_bits();\n"));
    }
    for (address, value) in &e.stores {
        e.code.push_str(&format!(
            "cpu.fp_lift_store_word({address},{value}).unwrap();\n"
        ));
    }
    let (next, cycles) = if let Some((target, condition, extra)) = &e.branch {
        (
            format!("if {condition}{{{target}}}else{{{at}}}"),
            format!("{}+if {condition}{{{extra}}}else{{0}}", e.cycles),
        )
    } else {
        (at.to_string(), e.cycles.to_string())
    };
    e.code.push_str(&format!(
        "cpu.set_pc({next});cpu.instruction_count+={};cpu.lifted_instructions+={};\n",
        instructions.len(),
        instructions.len()
    ));
    e.code.push_str(
        "#[cfg(test)] assert_eq!((cpu.fp_regs,cpu.fpscr,cpu.get_pc(),cpu.instruction_count,vec![",
    );
    for (address, _) in &e.stores {
        e.code
            .push_str(&format!("cpu.fp_lift_load({address}).unwrap(),"));
    }
    e.code.push_str(&format!("],cpu.r0_12,cpu.psr.value,{cycles},cpu.lr),expected,\"lifted ARM block at {pc:08x} differs from interpretation\");\n"));
    let readiness = if e.regs.is_empty() && e.dirty.is_empty() && !e.uses_fp {
        "compiled_block_ready"
    } else {
        "fp_lift_ready"
    };
    let loop_cycles = e.cycles + e.branch.as_ref().map_or(0, |(_, _, extra)| *extra);
    // Committed FP results remain ordinary. Register inputs need validation
    // only on entry; newly loaded memory words are still checked every time.
    let inputs = e
        .initial_fp
        .iter()
        .map(|index| format!("&&lift_ordinary(f32::from_bits(cpu.fp_regs[{index}]))"))
        .collect::<String>();
    let repetitions = if e
        .branch
        .as_ref()
        .is_some_and(|(target, _, _)| *target == pc)
    {
        (64 / loop_cycles).min(8)
    } else {
        1
    };
    if repetitions >= 2 {
        e.code.push_str(&format!("lifted_cycles+={cycles};lifted_any=true;if cpu.get_pc()!={pc}{{break 'lift;}}}}else{{break 'lift;}}\n"));
        let body = e.code.replace("return false;", "break 'lift;");
        Some(format!("if cpu.{readiness}(){inputs}{{let mut lifted_cycles=0u32;let mut lifted_any=false;'lift:for _ in 0..{repetitions}{{{body}}}if lifted_any{{cpu.finish_compiled_block(lifted_cycles);return true;}}}}\n"))
    } else {
        e.code.push_str(&format!(
            "cpu.finish_compiled_block({cycles});return true;}}\n"
        ));
        Some(format!("if cpu.{readiness}(){inputs}{{{}}}\n", e.code))
    }
}
pub const HELPERS: &str = r#"
#[inline(always)]fn lift_ordinary(value:f32)->bool{let bits=value.to_bits()&0x7fff_ffff;bits.wrapping_sub(0x0080_0000)<0x7f00_0000||bits==0}
#[cold]#[inline(never)]fn lift_zero_add(value:f32,a:f32,b:f32)->bool{value==0.&&a == -b}
#[cold]#[inline(never)]fn lift_zero_sub(value:f32,a:f32,b:f32)->bool{value==0.&&a==b}
#[cold]#[inline(never)]fn lift_zero_mul(value:f32,a:f32,b:f32)->bool{value==0.&&(a==0.||b==0.)}
#[cold]#[inline(never)]fn lift_zero_div(value:f32,a:f32,b:f32)->bool{value==0.&&a==0.&&b!=0.}
"#;
