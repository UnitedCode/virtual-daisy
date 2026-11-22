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
    if let Instruction::VLDM_T2 { params } = instruction {
        let registers = (&params.list)
            .into_iter()
            .map(|reg| format!("{reg:?}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("Instruction::VLDM_T2 {{ params: VStoreMultipleParams32 {{ mode: AddressingMode::{:?}, rn: Reg::{:?}, write_back: {}, list: [{registers}].into_iter().collect(), imm32: {} }} }}",params.mode,params.rn,params.write_back,params.imm32)
    } else {
        format!("Instruction::{instruction:?}")
    }
}
fn eligible(i: &Instruction) -> bool {
    matches!(
        i,
        Instruction::VADD_f32 { .. }
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
        )
}
fn terminal(i: &Instruction) -> bool {
    matches!(
        i,
        Instruction::B_t13 { .. }
            | Instruction::B_t24 { .. }
            | Instruction::CBZ { .. }
            | Instruction::CBNZ { .. }
    )
}
fn main() {
    println!("cargo:rerun-if-changed=../firmware/synthphone.bin");
    println!("cargo:rerun-if-changed=fp_lift.rs");
    println!("cargo:rerun-if-changed=hotspots.txt");
    let image = fs::read("../firmware/synthphone.bin").unwrap();
    let hash = Sha256::digest(&image);
    let mut out = format!("pub const IMAGE_HASH:[u8;32]={:?};\n", hash.as_slice());
    out.push_str("use zmu_cortex_m::{Processor,core::{condition::Condition::*,instruction::{*,SetFlags::*,SRType::*,Imm32Carry::*},register::{BaseReg,Reg,Reg::*,SingleReg::*,DoubleReg::*,ExtensionReg::*}}};\n");
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
        while offset + 4 <= image.len() && instructions.len() < 16 {
            if offset != start && entries.contains(&(0x08000000 + offset as u32)) {
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
            if !straight(&instruction) && !terminal(&instruction) {
                break;
            }
            let last = terminal(&instruction);
            let size = instruction_size(&instruction);
            instructions.push((instruction, size));
            offset += size;
            if last {
                break;
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
    for (_, start, instructions) in blocks.into_iter().take(512) {
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
    std::panic::set_hook(hook);
    out.push_str("_=>false}}\n#[inline(always)] pub fn contains(pc:u32)->bool {INDEX.get(pc.wrapping_sub(0x08000000)as usize/2).is_some_and(|i|*i!=0)}\n");
    out.push_str(&functions);
    out.push_str(&format!("static INDEX:[u16;65536]={index:?};\n"));
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("compiled.rs"),
        out,
    )
    .unwrap();
    println!(
        "cargo:warning=Prepared {count} ARM instruction blocks from the immutable firmware image"
    );
}
