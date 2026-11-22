//! Ahead-of-time decode immutable ARM bytes into short CPU instruction blocks.
//! This translates instructions; it does not compile a host DSP replacement.
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};
use zmu_cortex_m::{
    core::{
        instruction::{instruction_size, Instruction},
        thumb::ThumbCode,
    },
    decoder::Decoder,
    Processor,
};
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
        )
}
fn main() {
    println!("cargo:rerun-if-changed=../firmware/synthphone.bin");
    let image = fs::read("../firmware/synthphone.bin").unwrap();
    let hash = Sha256::digest(&image);
    let mut out = format!("pub const IMAGE_HASH:[u8;32]={:?};\n", hash.as_slice());
    out.push_str("use zmu_cortex_m::{Processor,core::{instruction::{*,SetFlags::*,SRType::*,Imm32Carry::*},register::{Reg::*,SingleReg::*,DoubleReg::*,ExtensionReg::*}}};\npub fn dispatch(cpu:&mut Processor,pc:u32)->bool {let Some(&index)=INDEX.get(pc.wrapping_sub(0x08000000)as usize/2)else{return false;};if index==0||!cpu.compiled_block_ready(){return false;}match index {\n");
    let c = Processor::new();
    let mut offset = 0;
    let mut count = 0;
    let mut index = [0u16; 65536];
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    while offset + 4 <= image.len() && count < 1023 {
        let start = offset;
        let mut instructions = Vec::new();
        while offset + 4 <= image.len() && instructions.len() < 16 {
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
            if !straight(&instruction) {
                break;
            }
            let size = instruction_size(&instruction);
            instructions.push((instruction, size));
            offset += size;
        }
        if instructions.len() >= 4 && instructions.iter().filter(|(i, _)| eligible(i)).count() >= 2
        {
            count += 1;
            index[start / 2] = count;
            let pc = 0x08000000 + start as u32;
            out.push_str(&format!("{count}=>{{let mut cycles=0;\n"));
            let mut next = pc;
            for (i, size) in &instructions {
                next += *size as u32;
                out.push_str(&format!("cycles+=cpu.execute_compiled(&Instruction::{i:?},{size});if !cpu.compiled_block_continues({next}){{cpu.finish_compiled_block(cycles);return true;}}\n"));
            }
            out.push_str("cpu.finish_compiled_block(cycles);true},\n");
        }
        if offset == start {
            offset += 2;
        }
    }
    std::panic::set_hook(hook);
    out.push_str("_=>false}}\n#[inline(always)] pub fn contains(pc:u32)->bool {INDEX.get(pc.wrapping_sub(0x08000000)as usize/2).is_some_and(|i|*i!=0)}\n");
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
