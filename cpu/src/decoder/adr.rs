use crate::core::instruction::{Instruction, RegImmParams};
use crate::core::{bits::Bits, register::Reg};

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_ADR_t1(command: u16) -> Instruction {
    Instruction::ADR {
        params: RegImmParams {
            r: From::from(command.get_bits(8..11)),
            imm32: u32::from(command.get_bits(0..8)) << 2,
        },
        add: true,
        thumb32: false,
    }
}

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_ADR_t2(opcode: u32) -> Instruction {
    decode_wide(opcode, false)
}

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_ADR_t3(opcode: u32) -> Instruction {
    decode_wide(opcode, true)
}

fn decode_wide(opcode: u32, add: bool) -> Instruction {
    let rd = Reg::from(opcode.get_bits(8..12) as u8);
    if !cfg!(any(feature = "armv7m", feature = "armv7em")) || matches!(rd, Reg::SP | Reg::PC) {
        return super::decode_UDF_t2(opcode);
    }
    Instruction::ADR {
        params: RegImmParams {
            r: rd,
            imm32: (u32::from(opcode.get_bit(26)) << 11)
                | (opcode.get_bits(12..15) << 8)
                | opcode.get_bits(0..8),
        },
        add,
        thumb32: true,
    }
}
