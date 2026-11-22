//! Ahead-of-time decode immutable ARM bytes into short CPU instruction blocks.
//! This translates instructions; it does not compile a host DSP replacement.
use sha2::{Digest, Sha256};
mod fp_lift;
use std::{env, fs, path::PathBuf};
use zmu_cortex_m::{
    core::{
        instruction::{instruction_size, Instruction},
        thumb::ThumbCode,
    },
    decoder::Decoder,
    Processor,
};
fn instruction_expr(instruction: &Instruction) -> String {
    fn list<T: std::fmt::Debug>(registers: impl IntoIterator<Item = T>) -> String {
        let registers = registers
            .into_iter()
            .map(|r| format!("{r:?}"))
            .collect::<Vec<_>>();
        if registers.is_empty() {
            "Default::default()".into()
        } else {
            format!("[{}].into_iter().collect()", registers.join(","))
        }
    }
    match instruction {
        Instruction::UDF { imm32, opcode, thumb32 } => format!("Instruction::UDF {{imm32:{imm32},opcode:zmu_cortex_m::core::thumb::ThumbCode::{opcode:?},thumb32:{thumb32}}}"),
        Instruction::VMRS { rt } => format!("Instruction::VMRS {{rt:VMRSTarget::{rt:?}}}"),
        Instruction::VLDM_T2 { params } | Instruction::VSTM_T2 { params } => {
            let name = if matches!(instruction, Instruction::VLDM_T2 { .. }) {
                "VLDM_T2"
            } else {
                "VSTM_T2"
            };
            format!("Instruction::{name} {{ params: VStoreMultipleParams32 {{ mode: AddressingMode::{:?}, rn: Reg::{:?}, write_back: {}, list: {}, imm32: {} }} }}",params.mode,params.rn,params.write_back,list(&params.list),params.imm32)
        }
        Instruction::VLDM_T1 { params } | Instruction::VSTM_T1 { params } => {
            let name = if matches!(instruction, Instruction::VLDM_T1 { .. }) {
                "VLDM_T1"
            } else {
                "VSTM_T1"
            };
            format!("Instruction::{name} {{ params: VStoreMultipleParams64 {{ mode: AddressingMode::{:?}, rn: Reg::{:?}, write_back: {}, list: {}, imm32: {} }} }}",params.mode,params.rn,params.write_back,list(&params.list),params.imm32)
        }
        Instruction::VPUSH { params } | Instruction::VPOP { params } => {
            let name = if matches!(instruction, Instruction::VPUSH { .. }) {
                "VPUSH"
            } else {
                "VPOP"
            };
            format!("Instruction::{name} {{params: VPushPopParams {{single_regs:{},single_precision_registers:{},double_precision_registers:{},imm32:{}}}}}",params.single_regs,list(&params.single_precision_registers),list(&params.double_precision_registers),params.imm32)
        }
        Instruction::PUSH { registers, thumb32 } | Instruction::POP { registers, thumb32 } => {
            let name = if matches!(instruction, Instruction::PUSH { .. }) {
                "PUSH"
            } else {
                "POP"
            };
            format!(
                "Instruction::{name} {{registers:{},thumb32:{thumb32}}}",
                list(registers)
            )
        }
        Instruction::STM { params, thumb32 } | Instruction::LDM { params, thumb32 } => {
            let name = if matches!(instruction, Instruction::STM { .. }) {
                "STM"
            } else {
                "LDM"
            };
            format!("Instruction::{name} {{params:LoadAndStoreMultipleParams {{rn:Reg::{:?},registers:{},wback:{}}},thumb32:{thumb32}}}",params.rn,list(&params.registers),params.wback)
        }
        Instruction::STMDB { params } => {
            format!("Instruction::STMDB {{params:LoadAndStoreMultipleParams {{rn:Reg::{:?},registers:{},wback:{}}}}}",params.rn,list(&params.registers),params.wback)
        }
        _ => format!("Instruction::{instruction:?}"),
    }
}
fn eligible(i: &Instruction) -> bool {
    matches!(
        i,
        Instruction::VADD_f32 { .. }
            | Instruction::VSQRT_f32 { .. }
            | Instruction::VSUB_f32 { .. }
            | Instruction::VMUL_f32 { .. }
            | Instruction::VDIV_f32 { .. }
            | Instruction::VMLA_f32 { .. }
            | Instruction::VMOV_reg_f32 { .. }
            | Instruction::VMOV_imm_32 { .. }
            | Instruction::VABS_f32 { .. }
            | Instruction::VNEG_f32 { .. }
            | Instruction::VLDR { .. }
            | Instruction::VSTR { .. }
            | Instruction::VLDM_T2 { .. }
    )
}
fn straight(i: &Instruction) -> bool {
    eligible(i)
        || matches!(
            i,
            Instruction::ADD_imm { .. }
                | Instruction::ADD_reg { .. }
                | Instruction::SUB_imm { .. }
                | Instruction::SUB_reg { .. }
                | Instruction::MOV_imm { .. }
                | Instruction::MOV_reg { .. }
                | Instruction::MOVT { .. }
                | Instruction::AND_imm { .. }
                | Instruction::AND_reg { .. }
                | Instruction::ORR_imm { .. }
                | Instruction::ORR_reg { .. }
                | Instruction::EOR_imm { .. }
                | Instruction::EOR_reg { .. }
                | Instruction::LSL_imm { .. }
                | Instruction::LSR_imm { .. }
                | Instruction::ASR_imm { .. }
                | Instruction::CMP_imm { .. }
                | Instruction::CMP_reg { .. }
                | Instruction::LDR_imm { .. }
                | Instruction::LDR_reg { .. }
                | Instruction::STR_imm { .. }
                | Instruction::STR_reg { .. }
                | Instruction::DSP { .. }
                | Instruction::RSB_imm { .. }
                | Instruction::RSB_reg { .. }
                | Instruction::LSL_reg { .. }
                | Instruction::LSR_reg { .. }
                | Instruction::ASR_reg { .. }
                | Instruction::UMULL { .. }
                | Instruction::CLZ { .. }
                | Instruction::BFC { .. }
                | Instruction::BFI { .. }
                | Instruction::STRD_imm { .. }
                | Instruction::LDRB_imm { .. }
                | Instruction::IT { .. }
                | Instruction::ADC_imm { .. }
                | Instruction::ADC_reg { .. }
                | Instruction::SBC_imm { .. }
                | Instruction::SBC_reg { .. }
                | Instruction::UBFX { .. }
                | Instruction::UMLAL { .. }
                | Instruction::BIC_imm { .. }
                | Instruction::BIC_reg { .. }
                | Instruction::LDRD_imm { .. }
        )
}
fn terminal(i: &Instruction) -> bool {
    matches!(
        i,
        Instruction::B_t13 { .. }
            | Instruction::B_t24 { .. }
            | Instruction::CBZ { .. }
            | Instruction::CBNZ { .. }
            | Instruction::BL { .. }
            | Instruction::BLX { .. }
            | Instruction::BX { .. }
            | Instruction::TBB { .. }
            | Instruction::TBH { .. }
    )
}
fn main() {
    println!("cargo:rerun-if-changed=../firmware/synthphone.bin");
    println!("cargo:rerun-if-changed=fp_lift.rs");
    println!("cargo:rerun-if-changed=hotspots.txt");
    println!("cargo:rerun-if-changed=fallback-hotspots.txt");
    println!("cargo:rerun-if-env-changed=VD_INTERPRETER_PROFILE");
    let image = fs::read("../firmware/synthphone.bin").unwrap();
    let hash = Sha256::digest(&image);
    let mut out = format!("pub const IMAGE_HASH:[u8;32]={:?};\n", hash.as_slice());
    if env::var("VD_INTERPRETER_PROFILE").as_deref() == Ok("1") {
        assert!(env::var_os("CARGO_FEATURE_PROFILE").is_some(), "Interpreter-only profiling requires the profile feature; never use it for a site build");
        out.push_str("pub fn dispatch(_: &mut zmu_cortex_m::Processor,_:u32,_:u64)->bool{false}\npub fn contains(_:u32)->bool{false}\npub fn single(_: &mut zmu_cortex_m::Processor,_:u32)->bool{false}\n");
        fs::write(
            PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("compiled.rs"),
            out,
        )
        .unwrap();
        return;
    }
    out.push_str("use zmu_cortex_m::{Processor,core::{condition::Condition::*,instruction::{*,ITCondition::*,SetFlags::*,SRType::*,Imm32Carry::*},register::{BaseReg,Reg,Reg::*,SingleReg::*,DoubleReg::*,ExtensionReg::*}}};\n");
    out.push_str(fp_lift::HELPERS);
    out.push_str("pub fn dispatch(cpu:&mut Processor,mut pc:u32,budget:u64)->bool {let start=cpu.cycle_count;let mut executed=false;while cpu.cycle_count-start+64<=budget {if !dispatch_one(cpu,pc){break;}executed=true;pc=cpu.get_pc();}executed}\nfn dispatch_one(cpu:&mut Processor,pc:u32)->bool {let Some(&index)=INDEX.get(pc.wrapping_sub(0x08000000)as usize/2)else{return false;};if index==0||!cpu.compiled_block_ready(){return false;}match index {\n");
    let c = Processor::new();
    let mut offset = 0;
    let mut count = 0;
    let counts = fs::read_to_string("hotspots.txt").unwrap();
    let mut visits = std::collections::BTreeMap::new();
    for line in counts.lines().filter(|s| !s.starts_with('#')) {
        let mut fields = line.split_whitespace();
        if let (Some(pc), Some(n)) = (fields.next(), fields.next()) {
            visits.insert(
                u32::from_str_radix(pc, 16).unwrap(),
                n.parse::<u64>().unwrap(),
            );
        }
    }
    let mut blocks = Vec::new();
    let mut functions = String::new();
    let mut index = [0u16; 65536];
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut entries = std::collections::BTreeSet::new();
    for &pc in visits.keys() {
        let offset = pc.wrapping_sub(0x08000000) as usize;
        if offset + 4 > image.len() {
            continue;
        }
        let a = u16::from_le_bytes(image[offset..offset + 2].try_into().unwrap());
        let b = u16::from_le_bytes(image[offset + 2..offset + 4].try_into().unwrap());
        let code = if a >> 11 >= 0b11101 {
            ThumbCode::Thumb32 {
                opcode: (u32::from(a) << 16) | u32::from(b),
            }
        } else {
            ThumbCode::Thumb16 { opcode: a }
        };
        if let Ok(i) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.decode(code))) {
            let relative = match i {
                Instruction::B_t13 { params, .. } => Some(params.imm32),
                Instruction::B_t24 { imm32, .. } | Instruction::BL { imm32 } => Some(imm32),
                Instruction::CBZ { params } | Instruction::CBNZ { params } => {
                    Some(params.imm32 as i32)
                }
                _ => None,
            };
            if let Some(relative) = relative {
                entries.insert(pc.wrapping_add(4).wrapping_add_signed(relative));
                entries.insert(pc + instruction_size(&i) as u32);
            }
        }
    }
    while offset + 4 <= image.len() {
        let start = offset;
        let mut instructions = Vec::new();
        let mut continued_branch = false;
        let mut maximum_cycles = 0;
        while offset + 4 <= image.len() && instructions.len() < 32 {
            // Never manufacture a trace through literal pools or unobserved
            // halfwords. Other paths remain available to the interpreter.
            if !visits.contains_key(&(0x08000000 + offset as u32)) {
                break;
            }
            if offset != start
                && !continued_branch
                && entries.contains(&(0x08000000 + offset as u32))
            {
                break;
            }
            let a = u16::from_le_bytes(image[offset..offset + 2].try_into().unwrap());
            let b = u16::from_le_bytes(image[offset + 2..offset + 4].try_into().unwrap());
            let wide = a >> 11 >= 0b11101;
            let code = if wide {
                ThumbCode::Thumb32 {
                    opcode: (u32::from(a) << 16) | u32::from(b),
                }
            } else {
                ThumbCode::Thumb16 { opcode: a }
            };
            let instruction =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.decode(code)));
            let Ok(instruction) = instruction else {
                break;
            };
            // Stay within the scheduler's 64-cycle event horizon, including
            // the slower IT and load/store retirements. Longer traces let a
            // complete DSP loop remain register-local instead of splitting it.
            let upper_bound = match &instruction {
                Instruction::IT { .. } => 4,
                Instruction::B_t13 { .. }
                | Instruction::B_t24 { .. }
                | Instruction::CBZ { .. }
                | Instruction::CBNZ { .. } => 3,
                Instruction::VLDM_T2 { params } => params.list.len() as u32 + 2,
                Instruction::PUSH { registers, .. } | Instruction::POP { registers, .. } => {
                    registers.len() as u32 + 4
                }
                Instruction::LDM { params, .. }
                | Instruction::STM { params, .. }
                | Instruction::STMDB { params } => params.registers.len() as u32 + 4,
                Instruction::VPUSH { .. }
                | Instruction::VPOP { .. }
                | Instruction::VLDM_T1 { .. }
                | Instruction::VSTM_T1 { .. }
                | Instruction::VSTM_T2 { .. }
                | Instruction::SVC { .. } => 36,
                _ if straight(&instruction) => 2,
                _ => 4,
            };
            if maximum_cycles + upper_bound > 64 {
                break;
            }
            maximum_cycles += upper_bound;
            let forward_target = match &instruction {
                Instruction::B_t13 { params, .. } if params.imm32 >= 0 => {
                    Some((0x08000000 + offset as u32 + 4).wrapping_add_signed(params.imm32))
                }
                Instruction::CBZ { params } | Instruction::CBNZ { params } => {
                    Some(0x08000000 + offset as u32 + 4 + params.imm32)
                }
                _ => None,
            };
            let size = instruction_size(&instruction);
            continued_branch = forward_target.is_some_and(|target| {
                visits.get(&target).copied().unwrap_or(0)
                    < visits
                        .get(&(0x08000000 + offset as u32 + size as u32))
                        .copied()
                        .unwrap_or(0)
            });
            let last = terminal(&instruction) && !continued_branch;
            instructions.push((instruction, size));
            offset += size;
            if last {
                break;
            }
        }
        // Keep a supported prefix when a later opcode needs interpretation.
        // Otherwise a single unsupported operation discards an entire hot loop.
        if instructions.len() >= 2
            && fp_lift::emit(&instructions, 0x08000000 + start as u32).is_none()
        {
            if let Some(length) = (2..instructions.len()).rev().find(|length| {
                fp_lift::emit(&instructions[..*length], 0x08000000 + start as u32).is_some()
            }) {
                instructions.truncate(length);
                offset = start + instructions.iter().map(|(_, size)| size).sum::<usize>();
            }
        }
        if instructions.len() >= 2 {
            let mut at = 0x08000000 + start as u32;
            let score = instructions
                .iter()
                .map(|(_, size)| {
                    let n = visits.get(&at).copied().unwrap_or(0);
                    at += *size as u32;
                    n
                })
                .sum::<u64>();
            if score > 0 {
                blocks.push((score, start, instructions));
            }
        }
        if offset == start {
            offset += 2;
        }
    }
    blocks.sort_by_key(|(score, _, _)| std::cmp::Reverse(*score));
    for (_, start, instructions) in blocks.into_iter().take(1024) {
        count += 1;
        index[start / 2] = count;
        let pc = 0x08000000 + start as u32;
        out.push_str(&format!("{count}=>block_{count}(cpu),\n"));
        functions.push_str(&format!(
            "#[allow(unused_mut,unused_comparisons,unused_variables)]#[inline(never)] fn block_{count}(cpu:&mut Processor)->bool{{\n"
        ));
        if let Some(lifted) = fp_lift::emit(&instructions, pc) {
            functions.push_str(&lifted);
        }
        let mut next = pc;
        for (i, size) in &instructions {
            next += *size as u32;
            let expression = instruction_expr(i);
            functions.push_str(&format!("let cycles=cpu.execute_compiled(&{expression},{size});cpu.finish_compiled_block(cycles);if !cpu.compiled_block_continues({next}){{return true;}}\n"));
        }
        functions.push_str("true}\n");
    }
    out.push_str("_=>false}}\n#[inline(always)] pub fn contains(pc:u32)->bool {INDEX.get(pc.wrapping_sub(0x08000000)as usize/2).is_some_and(|i|*i!=0)}\n");
    out.push_str(&functions);
    out.push_str(&format!("static INDEX:[u16;65536]={index:?};\n"));
    // Specialize the canonical executor for individually hot opcodes as well.
    // These preserve arbitrary IT state, branches, MMIO and exception behavior.
    // They do not replace guest routines or implement host DSP algorithms.
    let mut scores = visits.clone();
    for line in fs::read_to_string("fallback-hotspots.txt")
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let mut fields = line.split_whitespace();
        if let (Some(pc), Some(n)) = (fields.next(), fields.next()) {
            *scores
                .entry(u32::from_str_radix(pc, 16).unwrap())
                .or_default() += n.parse::<u64>().unwrap() * 64;
        }
    }
    let mut candidates = scores.into_iter().collect::<Vec<_>>();
    candidates.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let mut singles = [0u16; 65536];
    let mut opcodes = std::collections::BTreeMap::new();
    let mut single_functions = String::new();
    let mut cases = String::new();
    out.push_str("pub fn single(cpu:&mut Processor,pc:u32)->bool{let Some(&i)=SINGLES.get(pc.wrapping_sub(0x08000000)as usize/2)else{return false;};match i{\n");
    for (pc, _) in candidates.into_iter().take(512) {
        let offset = pc.wrapping_sub(0x08000000) as usize;
        if offset + 4 > image.len() {
            continue;
        }
        let a = u16::from_le_bytes(image[offset..offset + 2].try_into().unwrap());
        let code = if a >> 11 >= 0b11101 {
            ThumbCode::Thumb32 {
                opcode: (u32::from(a) << 16)
                    | u32::from(u16::from_le_bytes(
                        image[offset + 2..offset + 4].try_into().unwrap(),
                    )),
            }
        } else {
            ThumbCode::Thumb16 { opcode: a }
        };
        let Ok(instruction) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.decode(code)))
        else {
            continue;
        };
        let expression = instruction_expr(&instruction);
        let size = instruction_size(&instruction);
        let next = opcodes.len() as u16 + 1;
        let n=*opcodes.entry(expression.clone()).or_insert_with(||{
            out.push_str(&format!("{next}=>single_{next}(cpu),\n"));
            single_functions.push_str(&format!("#[inline(never)]fn single_{next}(cpu:&mut Processor)->bool{{let cycles=cpu.execute_compiled(&{expression},{size});cpu.retire_decoded(cycles);true}}\n"));
            cases.push_str(&format!("assert_eq!(cpu.decode(ThumbCode::{code:?}),{expression});\n"));
            next
        });
        singles[offset / 2] = n;
    }
    out.push_str("_=>false}}\n");
    out.push_str(&single_functions);
    out.push_str(&format!("static SINGLES:[u16;65536]={singles:?};\n"));
    out.push_str("#[cfg(test)]#[test]fn decoded_literals_match_original_opcodes(){use zmu_cortex_m::{decoder::Decoder,core::thumb::ThumbCode};let cpu=Processor::new();\n");
    out.push_str(&cases);
    out.push_str("}\n");
    std::panic::set_hook(hook);
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("compiled.rs"),
        out,
    )
    .unwrap();
    println!(
        "cargo:warning=Prepared {count} ARM instruction blocks from the immutable firmware image"
    );
}
