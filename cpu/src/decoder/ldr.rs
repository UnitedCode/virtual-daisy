use crate::core::bits::Bits;
use crate::core::instruction::Instruction;
use crate::core::instruction::{
    Reg2DoubleParams, Reg2FullParams, Reg2RtRnImm32Params, Reg2RtRnParams, Reg3FullParams,
    RegImm32AddParams, SRType,
};
use crate::core::register::Reg;

pub(super) fn decode_literal(opcode: u32) -> Option<Instruction> {
    if !cfg!(any(feature = "armv7m", feature = "armv7em")) {
        return None;
    }
    Some(match opcode & 0xff7f0000 {
        0xf85f0000 => decode_LDR_lit_t2(opcode),
        0xf81f0000 => super::ldrb::decode_LDRB_lit_t1(opcode),
        0xf83f0000 => super::ldrh::decode_LDRH_lit_t1(opcode),
        0xf91f0000 => super::ldrsb::decode_LDRSB_lit_t1(opcode),
        0xf93f0000 => super::ldrsh::decode_LDRSH_lit_t1(opcode),
        0xe95f0000 => decode_LDRD_lit_t1(opcode),
        _ => return None,
    })
}

pub(super) fn narrow_literal_params(opcode: u32) -> Option<Reg2FullParams> {
    let rt = Reg::from(opcode.get_bits(12..16) as u8);
    if rt == Reg::SP {
        return None;
    }
    Some(Reg2FullParams {
        rt,
        rn: Reg::PC,
        imm32: opcode.get_bits(0..12),
        index: true,
        add: opcode.get_bit(23),
        wback: false,
    })
}

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_LDR_imm_t1(opcode: u16) -> Instruction {
    Instruction::LDR_imm {
        params: Reg2FullParams {
            rt: Reg::from(opcode.get_bits(0..3) as u8),
            rn: Reg::from(opcode.get_bits(3..6) as u8),
            imm32: u32::from(opcode.get_bits(6..11)) << 2,
            index: true,
            add: true,
            wback: false,
        },
        thumb32: false,
    }
}

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_LDR_imm_t2(opcode: u16) -> Instruction {
    Instruction::LDR_imm {
        params: Reg2FullParams {
            rt: From::from(opcode.get_bits(8..11) as u8),
            rn: Reg::SP,
            imm32: u32::from(opcode.get_bits(0..8)) << 2,
            index: true,
            add: true,
            wback: false,
        },
        thumb32: false,
    }
}

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_LDR_lit_t1(opcode: u16) -> Instruction {
    Instruction::LDR_lit {
        params: RegImm32AddParams {
            rt: Reg::from(opcode.get_bits(8..11) as u8),
            imm32: u32::from(opcode.get_bits(0..8)) << 2,
            add: true,
        },
        thumb32: false,
    }
}

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_LDR_reg_t1(opcode: u16) -> Instruction {
    Instruction::LDR_reg {
        params: Reg3FullParams {
            rt: Reg::from(opcode.get_bits(0..3) as u8),
            rn: Reg::from(opcode.get_bits(3..6) as u8),
            rm: Reg::from(opcode.get_bits(6..9) as u8),
            index: true,
            add: true,
            wback: false,
            shift_t: SRType::LSL,
            shift_n: 0,
        },
        thumb32: false,
    }
}

#[allow(non_snake_case)]
pub fn decode_LDR_imm_t3(opcode: u32) -> Instruction {
    // ARMv7-M

    Instruction::LDR_imm {
        params: Reg2FullParams {
            rt: From::from(opcode.get_bits(12..16) as u8),
            rn: From::from(opcode.get_bits(16..20) as u8),
            imm32: opcode.get_bits(0..12),
            index: true,
            add: true,
            wback: false,
        },
        thumb32: true,
    }
}

#[allow(non_snake_case)]
pub fn decode_LDR_imm_t4(opcode: u32) -> Instruction {
    // ARMv7-M
    Instruction::LDR_imm {
        params: Reg2FullParams {
            rt: From::from(opcode.get_bits(12..16) as u8),
            rn: From::from(opcode.get_bits(16..20) as u8),
            imm32: opcode.get_bits(0..8),
            index: opcode.get_bit(10),
            add: opcode.get_bit(9),
            wback: opcode.get_bit(8),
        },
        thumb32: true,
    }
}

#[allow(non_snake_case)]
pub fn decode_LDR_lit_t2(opcode: u32) -> Instruction {
    Instruction::LDR_lit {
        params: RegImm32AddParams {
            rt: Reg::from(opcode.get_bits(12..16) as u8),
            imm32: opcode.get_bits(0..12),
            add: opcode.get_bit(23),
        },
        thumb32: true,
    }
}

#[allow(non_snake_case)]
pub fn decode_LDR_reg_t2(opcode: u32) -> Instruction {
    Instruction::LDR_reg {
        params: Reg3FullParams {
            rm: Reg::from(opcode.get_bits(0..4) as u8),
            rt: Reg::from(opcode.get_bits(12..16) as u8),
            rn: Reg::from(opcode.get_bits(16..20) as u8),
            shift_t: SRType::LSL,
            shift_n: opcode.get_bits(4..6) as u8,
            index: true,
            add: true,
            wback: false,
        },
        thumb32: true,
    }
}

#[allow(non_snake_case)]
pub fn decode_LDRBT_t1(opcode: u32) -> Instruction {
    super::unprivileged::decode_UNPRIV_t1(opcode)
}

#[allow(non_snake_case)]
pub fn decode_LDRD_imm_t1(opcode: u32) -> Instruction {
    Instruction::LDRD_imm {
        params: Reg2DoubleParams {
            rt2: From::from(opcode.get_bits(8..12) as u8),
            rt: From::from(opcode.get_bits(12..16) as u8),
            rn: From::from(opcode.get_bits(16..20) as u8),
            imm32: opcode.get_bits(0..8) << 2,
            index: opcode.get_bit(24),
            add: opcode.get_bit(23),
            wback: opcode.get_bit(21),
        },
    }
}

#[allow(non_snake_case)]
pub fn decode_LDRD_lit_t1(opcode: u32) -> Instruction {
    let rt = Reg::from(opcode.get_bits(12..16) as u8);
    let rt2 = Reg::from(opcode.get_bits(8..12) as u8);
    if opcode & 0xff7f0000 != 0xe95f0000
        || rt == rt2
        || [rt, rt2].iter().any(|r| matches!(r, Reg::SP | Reg::PC))
    {
        return super::decode_UDF_t2(opcode);
    }
    Instruction::LDRD_imm {
        params: Reg2DoubleParams {
            rn: Reg::PC,
            rt,
            rt2,
            imm32: opcode.get_bits(0..8) << 2,
            index: true,
            add: opcode.get_bit(23),
            wback: false,
        },
    }
}

#[allow(non_snake_case)]
pub fn decode_LDREXB_t1(opcode: u32) -> Instruction {
    Instruction::LDREXB {
        params: Reg2RtRnParams {
            rt: From::from(opcode.get_bits(12..16) as u8),
            rn: From::from(opcode.get_bits(16..20) as u8),
        },
    }
}

#[allow(non_snake_case)]
pub fn decode_LDREXH_t1(opcode: u32) -> Instruction {
    Instruction::LDREXH {
        params: Reg2RtRnParams {
            rt: From::from(opcode.get_bits(12..16) as u8),
            rn: From::from(opcode.get_bits(16..20) as u8),
        },
    }
}

#[allow(non_snake_case)]
pub fn decode_LDREX_t1(opcode: u32) -> Instruction {
    Instruction::LDREX {
        params: Reg2RtRnImm32Params {
            rt: From::from(opcode.get_bits(12..16) as u8),
            rn: From::from(opcode.get_bits(16..20) as u8),
            imm32: opcode.get_bits(0..8) << 2,
        },
    }
}

#[allow(non_snake_case)]
pub fn decode_LDRHT_t1(opcode: u32) -> Instruction {
    super::unprivileged::decode_UNPRIV_t1(opcode)
}

#[allow(non_snake_case)]
pub fn decode_LDRSBT_t1(opcode: u32) -> Instruction {
    super::unprivileged::decode_UNPRIV_t1(opcode)
}

#[allow(non_snake_case)]
pub fn decode_LDRSHT(opcode: u32) -> Instruction {
    super::unprivileged::decode_UNPRIV_t1(opcode)
}

#[allow(non_snake_case)]
pub fn decode_LDRT_t1(opcode: u32) -> Instruction {
    super::unprivileged::decode_UNPRIV_t1(opcode)
}
