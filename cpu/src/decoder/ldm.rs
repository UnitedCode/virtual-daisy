use crate::core::bits::Bits;

use crate::core::instruction::{Instruction, LoadAndStoreMultipleParams};
use crate::core::operation::get_reglist;
use crate::core::register::Reg;

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_LDM_t1(opcode: u16) -> Instruction {
    let regs = get_reglist(opcode & 0b_1111_1111);
    let rn = Reg::from(opcode.get_bits(8..11) as u8);

    Instruction::LDM {
        params: LoadAndStoreMultipleParams {
            registers: regs,
            rn,
            wback: !regs.contains(&rn),
        },
        thumb32: false,
    }
}

#[allow(non_snake_case)]
pub fn decode_LDMDB_t1(opcode: u32) -> Instruction {
    decode_wide(opcode, true)
}

#[allow(non_snake_case)]
pub fn decode_LDM_t2(opcode: u32) -> Instruction {
    decode_wide(opcode, false)
}

fn decode_wide(opcode: u32, decrement_before: bool) -> Instruction {
    let regs = get_reglist((opcode & 0b1101_1111_1111_1111) as u16);
    let rn = Reg::from(opcode.get_bits(16..20) as u8);
    let wback = opcode.get_bit(21);

    if !cfg!(any(feature = "armv7m", feature = "armv7em"))
        || rn == Reg::PC
        || opcode.get_bit(13)
        || regs.len() < 2
        || (regs.contains(&Reg::LR) && regs.contains(&Reg::PC))
        || (wback && regs.contains(&rn))
    {
        return Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        };
    }

    let params = LoadAndStoreMultipleParams {
        registers: regs,
        rn,
        wback,
    };
    if decrement_before {
        Instruction::LDMDB { params }
    } else {
        Instruction::LDM {
            params,
            thumb32: true,
        }
    }
}
