use crate::core::{
    bits::Bits,
    instruction::{Instruction, SRType},
    register::Reg,
};

#[allow(non_snake_case)]
pub fn decode_SSAT_t1(opcode: u32) -> Instruction {
    let rd = Reg::from(opcode.get_bits(8..12) as u8);
    let rn = Reg::from(opcode.get_bits(16..20) as u8);
    let shift_n = (opcode.get_bits(6..8) + (opcode.get_bits(12..15) << 2)) as usize;
    let arithmetic = opcode.get_bit(21);
    if matches!(rd, Reg::SP | Reg::PC)
        || matches!(rn, Reg::SP | Reg::PC)
        || (arithmetic && shift_n == 0)
    {
        // ASR #0 is the separate DSP-only SSAT16 encoding.
        return Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        };
    }
    Instruction::SSAT {
        rd,
        rn,
        saturate_to: opcode.get_bits(0..5) + 1,
        shift_t: if arithmetic { SRType::ASR } else { SRType::LSL },
        shift_n,
    }
}

#[cfg(feature = "has-dsp-ext")]
#[allow(non_snake_case)]
pub fn decode_SAT16_t1(opcode: u32) -> Instruction {
    let rd = Reg::from(opcode.get_bits(8..12) as u8);
    let rn = Reg::from(opcode.get_bits(16..20) as u8);
    if matches!(rd, Reg::SP | Reg::PC) || matches!(rn, Reg::SP | Reg::PC) {
        return Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        };
    }
    let width = opcode.get_bits(0..4);
    if opcode.get_bit(23) {
        Instruction::USAT16 {
            rd,
            rn,
            saturate_to: width,
        }
    } else {
        Instruction::SSAT16 {
            rd,
            rn,
            saturate_to: width + 1,
        }
    }
}
