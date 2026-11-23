//! Core register restrictions and Armv7-M memory-hint encoding space.
use crate::core::{instruction::Instruction, register::Reg};

pub(super) fn memory_hint(opcode: u32) -> Option<bool> {
    if !cfg!(any(feature = "armv7m", feature = "armv7em")) || opcode & 0xfe50_f000 != 0xf810_f000 {
        return None;
    }
    let base = (opcode >> 16) & 15;
    let mode = (opcode >> 6) & 63;
    if base == 15 || opcode & (1 << 23) != 0 || mode & 0x3c == 0x30 {
        Some(true)
    } else if mode == 0 {
        // Unallocated halfword hints are NOPs. Defined PLD/PLI register
        // forms forbid SP/PC offsets; those unpredictable forms fault here.
        Some(opcode & (1 << 21) != 0 || !matches!(opcode & 15, 13 | 15))
    } else {
        Some(false)
    }
}

pub(super) fn invalid_pc_write(instruction: &Instruction) -> bool {
    match instruction {
        Instruction::ADD_imm { params, .. }
        | Instruction::ADC_imm { params }
        | Instruction::SBC_imm { params }
        | Instruction::SUB_imm { params, .. }
        | Instruction::RSB_imm { params, .. } => params.rd == Reg::PC,
        Instruction::AND_imm { params }
        | Instruction::BIC_imm { params }
        | Instruction::EOR_imm { params }
        | Instruction::ORR_imm { params }
        | Instruction::ORN_imm { params } => params.rd == Reg::PC,
        Instruction::ADD_reg {
            params,
            thumb32: true,
        }
        | Instruction::ADC_reg { params, .. }
        | Instruction::SBC_reg { params, .. }
        | Instruction::SUB_reg { params, .. }
        | Instruction::RSB_reg { params, .. }
        | Instruction::AND_reg { params, .. }
        | Instruction::BIC_reg { params, .. }
        | Instruction::EOR_reg { params, .. }
        | Instruction::ORR_reg { params, .. }
        | Instruction::ORN_reg { params } => params.rd == Reg::PC,
        Instruction::MOV_imm { params, .. } | Instruction::MVN_imm { params } => {
            params.rd == Reg::PC
        }
        Instruction::MVN_reg { params, .. }
        | Instruction::ADD_sp_reg {
            params,
            thumb32: true,
        } => params.rd == Reg::PC,
        Instruction::MOV_reg {
            params,
            thumb32: true,
        } => params.rd == Reg::PC,
        Instruction::LDRD_imm { params } => {
            params.rt == Reg::PC || params.rt2 == Reg::PC || (params.wback && params.rn == Reg::PC)
        }
        Instruction::ASR_imm { params, .. }
        | Instruction::LSL_imm { params, .. }
        | Instruction::LSR_imm { params, .. }
        | Instruction::ROR_imm { params } => params.rd == Reg::PC,
        Instruction::ASR_reg { params, .. }
        | Instruction::LSL_reg { params, .. }
        | Instruction::LSR_reg { params, .. }
        | Instruction::ROR_reg { params, .. }
        | Instruction::MUL { params, .. } => params.rd == Reg::PC,
        Instruction::RRX { params } => params.rd == Reg::PC,
        Instruction::MLA { params } | Instruction::MLS { params } => params.rd == Reg::PC,
        Instruction::SMLAL { params }
        | Instruction::SMULL { params }
        | Instruction::UMLAL { params }
        | Instruction::UMULL { params } => params.rdlo == Reg::PC || params.rdhi == Reg::PC,
        Instruction::BFC { params } => params.rd == Reg::PC,
        Instruction::BFI { params } => params.rd == Reg::PC,
        Instruction::SBFX { params } | Instruction::UBFX { params } => params.rd == Reg::PC,
        Instruction::CLZ { params }
        | Instruction::REV { params, .. }
        | Instruction::REV16 { params, .. }
        | Instruction::REVSH { params, .. } => params.rd == Reg::PC,
        Instruction::MOVT { params } => params.rd == Reg::PC,
        Instruction::ADR { params, .. } => params.r == Reg::PC,
        Instruction::MRS { params } => params.rd == Reg::PC,
        Instruction::SXTB { params, .. }
        | Instruction::SXTH { params, .. }
        | Instruction::UXTB { params, .. }
        | Instruction::UXTH { params, .. }
        | Instruction::UXTB16 { params } => params.rd == Reg::PC,
        Instruction::UXTAB { params } => params.rd == Reg::PC,
        Instruction::SEL { params } | Instruction::UADD8 { params } => params.rd == Reg::PC,
        Instruction::LDREX { params } => params.rt == Reg::PC,
        Instruction::LDREXB { params } | Instruction::LDREXH { params } => params.rt == Reg::PC,
        Instruction::STREX { params } => params.rd == Reg::PC,
        Instruction::STREXB { params } | Instruction::STREXH { params } => params.rd == Reg::PC,
        Instruction::LDR_imm { params, .. }
        | Instruction::STR_imm { params, .. }
        | Instruction::LDRB_imm { params, .. }
        | Instruction::STRB_imm { params, .. }
        | Instruction::LDRH_imm { params, .. }
        | Instruction::STRH_imm { params, .. }
        | Instruction::LDRSB_imm { params, .. }
        | Instruction::LDRSH_imm { params, .. } => params.wback && params.rn == Reg::PC,
        Instruction::LDR_reg { params, .. }
        | Instruction::STR_reg { params, .. }
        | Instruction::LDRB_reg { params, .. }
        | Instruction::STRB_reg { params, .. }
        | Instruction::LDRH_reg { params, .. }
        | Instruction::STRH_reg { params, .. }
        | Instruction::LDRSB_reg { params, .. }
        | Instruction::LDRSH_reg { params, .. } => params.wback && params.rn == Reg::PC,
        Instruction::STRD_imm { params } => params.wback && params.rn == Reg::PC,
        Instruction::LDM { params, .. }
        | Instruction::LDMDB { params }
        | Instruction::STM { params, .. }
        | Instruction::STMDB { params } => params.rn == Reg::PC,
        _ => false,
    }
}
