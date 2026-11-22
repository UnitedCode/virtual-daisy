//! Lift short pure-VFP sequences into register-local Rust expressions.
//! Values commit after all speculative operations pass their IEEE guards.
use std::collections::{BTreeMap, BTreeSet};
use zmu_cortex_m::core::{
    instruction::{AddressingMode, Imm32Carry, Instruction, SRType, SetFlags},
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
}
impl Emitter {
    fn logic_flags(&mut self, value: &str, carry: &str) {
        self.status_dirty = true;
        self.code.push_str(&format!("_status=(_status&0x1fff_ffff)|({value}&0x8000_0000)|((({value}==0)as u32)<<30)|(({carry} as u32)<<29);\n"));
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
        if self.stored {
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
        if reg as usize >= 13 {
            return None;
        }
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
            self.status_dirty = true;
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
            self.code.push_str(&format!("_status=(_status&0x0fff_ffff)|({v}&0x8000_0000)|((({v}==0)as u32)<<30)|(({carry})<<29)|(({overflow})<<28);\n"));
        }
        v
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
        self.code.push_str(&format!(
            "let {v}=f32::from_bits(cpu.fp_regs[{index}]);valid&=lift_ordinary({v});\n"
        ));
        self.regs.insert(index, v.clone());
        v
    }
    fn dest(&mut self, index: usize, value: String) {
        self.regs.insert(index, value);
        self.dirty.insert(index);
    }
    fn math(&mut self, op: u8, a: &str, b: &str) -> String {
        let v = self.name();
        let operator = ["+", "-", "*", "/"][op as usize];
        self.code.push_str(&format!("let {v}={a}{operator}{b};\n"));
        let zero = match op {
            0 => format!("({a} == -{b})"),
            1 => format!("({a} == {b})"),
            2 => format!("(({a}==0.)|({b}==0.))"),
            _ => format!("(({a}==0.)&({b}!=0.))"),
        };
        self.code.push_str(&format!("let z{v}=({v}==0.)&{zero};let b{v}={v}.to_bits()&0x7fff_ffff;valid&=(((b{v}>0x0080_0000)&(b{v}<0x7f80_0000))|z{v})&(_sticky|z{v});\n"));
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
            "if cpu.fp_lift_ready(){let _sticky=cpu.fpscr&16!=0;let mut valid=true;#[allow(unused_mut)]let mut _status=cpu.psr.value;\n",
        ),
        regs: BTreeMap::new(),
        dirty: BTreeSet::new(),
        stores: Vec::new(),
        next: 0,
        stored: false,
        gprs:BTreeMap::new(),gpr_dirty:BTreeSet::new(),status_dirty:false,cycles:0,
    };
    let mut at = pc;
    for (i, size) in instructions {
        e.cycles += 1;
        match i {
            Instruction::MOV_imm { params, .. } => {
                let (value, carry) = e.immediate(params.imm32);
                if params.setflags != SetFlags::False {
                    e.logic_flags(&value, &carry);
                }
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::MOV_reg { params, .. } => {
                let value = e.gpr(params.rm)?;
                if params.setflags != SetFlags::False {
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
            Instruction::AND_imm { params }
            | Instruction::ORR_imm { params }
            | Instruction::EOR_imm { params } => {
                let left = e.gpr(params.rn)?;
                let (right, carry) = e.immediate(params.imm32);
                let operator = match i {
                    Instruction::AND_imm { .. } => "&",
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
            | Instruction::ORR_reg { params, .. }
            | Instruction::EOR_reg { params, .. } => {
                let left = e.gpr(params.rn)?;
                let rm = e.gpr(params.rm)?;
                let (right, carry) = e.shift(&rm, params.shift_t, params.shift_n);
                let operator = match i {
                    Instruction::AND_reg { .. } => "&",
                    Instruction::ORR_reg { .. } => "|",
                    _ => "^",
                };
                let value = e.name();
                e.code
                    .push_str(&format!("let {value}={left}{operator}{right};\n"));
                if params.setflags != SetFlags::False {
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
                if params.setflags != SetFlags::False {
                    e.logic_flags(&value, &carry);
                }
                e.gpr_dest(params.rd, value)?;
            }
            Instruction::LDR_imm { params, .. } | Instruction::STR_imm { params, .. } => {
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
                if e.stored {
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
            Instruction::ADD_imm { params, .. } | Instruction::SUB_imm { params, .. } => {
                let a = e.gpr(params.rn)?;
                let b = format!("{}u32", params.imm32);
                let v = e.integer_math(
                    &a,
                    &b,
                    matches!(i, Instruction::SUB_imm { .. }),
                    params.setflags != SetFlags::False,
                );
                e.gpr_dest(params.rd, v)?;
            }
            Instruction::ADD_reg { params, .. } | Instruction::SUB_reg { params, .. } => {
                if params.shift_t != SRType::LSL || params.shift_n >= 32 {
                    return None;
                }
                let a = e.gpr(params.rn)?;
                let rm = e.gpr(params.rm)?;
                let shifted = e.name();
                e.code.push_str(&format!(
                    "let {shifted}={rm}.wrapping_shl({});\n",
                    params.shift_n
                ));
                let v = e.integer_math(
                    &a,
                    &shifted,
                    matches!(i, Instruction::SUB_reg { .. }),
                    params.setflags != SetFlags::False,
                );
                e.gpr_dest(params.rd, v)?;
            }
            Instruction::CMP_imm { params, .. } => {
                let a = e.gpr(params.r)?;
                let b = format!("{}u32", params.imm32);
                e.integer_math(&a, &b, true, true);
            }
            Instruction::CMP_reg { params, .. } => {
                if params.shift_t != SRType::LSL || params.shift_n >= 32 {
                    return None;
                }
                let a = e.gpr(params.rn)?;
                let rm = e.gpr(params.rm)?;
                let shifted = e.name();
                e.code.push_str(&format!(
                    "let {shifted}={rm}.wrapping_shl({});\n",
                    params.shift_n
                ));
                e.integer_math(&a, &shifted, true, true);
            }
            Instruction::VLDR { params } => {
                if e.stored {
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
                let d = if *negate { format!("(-{d})") } else { d };
                let product = if *subtract {
                    format!("(-{product})")
                } else {
                    product
                };
                let v = e.math(0, &d, &product);
                e.dest(params.sd as usize, v);
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
    }
    e.code.push_str("if valid{\n");
    // In native unit tests each accepted block is shadowed by ordinary execution,
    // including all FP bits, flags, changed memory, PC and retired instruction count.
    e.code.push_str("#[cfg(test)] let expected={let old_fp=cpu.fp_regs;let old_status=cpu.fpscr;let old_pc=cpu.get_pc();let old_count=cpu.instruction_count;let old_gprs=cpu.r0_12;let old_psr=cpu.psr.value;\n");
    for (n, (address, _)) in e.stores.iter().enumerate() {
        e.code.push_str(&format!(
            "let old{n}=cpu.fp_lift_load({address}).unwrap();\n"
        ));
    }
    for (i, size) in instructions {
        let expression = super::instruction_expr(i);
        e.code.push_str(&format!(
            "zmu_cortex_m::executor::Executor::execute(cpu,&{expression},{size});\n"
        ));
    }
    e.code
        .push_str("let result:([u32;32],u32,u32,u64,Vec<u32>,[u32;13],u32)=(cpu.fp_regs,cpu.fpscr,cpu.get_pc(),cpu.instruction_count,vec![");
    for (address, _) in &e.stores {
        e.code
            .push_str(&format!("cpu.fp_lift_load({address}).unwrap(),"));
    }
    e.code.push_str("],cpu.r0_12,cpu.psr.value);cpu.fp_regs=old_fp;cpu.fpscr=old_status;cpu.set_pc(old_pc);cpu.instruction_count=old_count;cpu.r0_12=old_gprs;cpu.psr.value=old_psr;\n");
    for (n, (address, _)) in e.stores.iter().enumerate() {
        e.code.push_str(&format!(
            "cpu.fp_lift_store_word({address},old{n}).unwrap();\n"
        ));
    }
    e.code.push_str("result};\n");
    for index in &e.gpr_dirty {
        let value = &e.gprs[index];
        e.code.push_str(&format!("cpu.r0_12[{index}]={value};\n"));
    }
    if e.status_dirty {
        e.code.push_str("cpu.psr.value=_status;\n");
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
    e.code.push_str(&format!(
        "cpu.set_pc({at});cpu.instruction_count+={};cpu.lifted_instructions+={};\n",
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
    e.code.push_str(
        "],cpu.r0_12,cpu.psr.value),expected,\"lifted VFP block differs from interpretation\");\n",
    );
    e.code.push_str(&format!(
        "cpu.finish_compiled_block({});return true;}}}}\n",
        e.cycles
    ));
    Some(e.code)
}
pub const HELPERS:&str="#[inline(always)]fn lift_ordinary(value:f32)->bool{let bits=value.to_bits();let exponent=bits&0x7f80_0000;(exponent!=0x7f80_0000)&((exponent!=0)|(bits&0x7f_ffff==0))}\n";
