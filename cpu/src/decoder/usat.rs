use crate::core::instruction::Instruction;
use crate::core::{bits::Bits, instruction::SRType, register::Reg};

#[allow(non_snake_case)]
pub fn decode_USAT_t1(opcode: u32) -> Instruction {
    let rd = Reg::from(opcode.get_bits(8..12) as u8);
    let rn = Reg::from(opcode.get_bits(16..20) as u8);
    let amount = (opcode.get_bits(6..8) + (opcode.get_bits(12..15) << 2)) as usize;
    let arithmetic = opcode.get_bit(21);
    if matches!(rd, Reg::SP | Reg::PC)
        || matches!(rn, Reg::SP | Reg::PC)
        || (arithmetic && amount == 0)
    {
        return Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        };
    }
    Instruction::USAT {
        rd,
        rn,
        saturate_to: opcode.get_bits(0..5),
        shift_t: if arithmetic { SRType::ASR } else { SRType::LSL },
        shift_n: amount,
    }
}
