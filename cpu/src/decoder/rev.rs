use crate::core::bits::Bits;
use crate::core::instruction::{Instruction, Reg2RdRmParams};
use crate::core::register::Reg;

fn wide_params(opcode: u32) -> Option<Reg2RdRmParams> {
    let rd = Reg::from(opcode.get_bits(8..12) as u8);
    let rm = Reg::from(opcode.get_bits(0..4) as u8);
    if !cfg!(any(feature = "armv7m", feature = "armv7em"))
        || opcode.get_bits(16..20) != opcode.get_bits(0..4)
        || [rd, rm].iter().any(|r| matches!(r, Reg::SP | Reg::PC))
    {
        return None;
    }
    Some(Reg2RdRmParams { rd, rm })
}

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_REVSH_t1(opcode: u16) -> Instruction {
    Instruction::REVSH {
        params: Reg2RdRmParams {
            rd: From::from(opcode.get_bits(0..3)),
            rm: From::from(opcode.get_bits(3..6)),
        },
        thumb32: false,
    }
}

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_REV_t1(opcode: u16) -> Instruction {
    Instruction::REV {
        params: Reg2RdRmParams {
            rd: From::from(opcode.get_bits(0..3)),
            rm: From::from(opcode.get_bits(3..6)),
        },
        thumb32: false,
    }
}

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_REV16_t1(opcode: u16) -> Instruction {
    Instruction::REV16 {
        params: Reg2RdRmParams {
            rd: From::from(opcode.get_bits(0..3)),
            rm: From::from(opcode.get_bits(3..6)),
        },
        thumb32: false,
    }
}

#[allow(non_snake_case)]
pub fn decode_REV16_t2(opcode: u32) -> Instruction {
    match wide_params(opcode) {
        Some(params) => Instruction::REV16 {
            params,
            thumb32: true,
        },
        None => super::decode_UDF_t2(opcode),
    }
}

#[allow(non_snake_case)]
pub fn decode_REVSH_t2(opcode: u32) -> Instruction {
    match wide_params(opcode) {
        Some(params) => Instruction::REVSH {
            params,
            thumb32: true,
        },
        None => super::decode_UDF_t2(opcode),
    }
}

#[allow(non_snake_case)]
pub fn decode_REV_t2(opcode: u32) -> Instruction {
    match wide_params(opcode) {
        Some(params) => Instruction::REV {
            params,
            thumb32: true,
        },
        None => super::decode_UDF_t2(opcode),
    }
}
