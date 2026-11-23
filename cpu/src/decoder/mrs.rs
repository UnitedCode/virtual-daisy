use crate::core::{
    bits::Bits,
    instruction::{Instruction, MrsParams},
    register::Reg,
};

#[allow(non_snake_case)]
pub fn decode_MRS_t1(opcode: u32) -> Instruction {
    let reg_rd: u8 = opcode.get_bits(8..12) as u8;
    if reg_rd == 13 || reg_rd == 15 {
        return Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        };
    }
    Instruction::MRS {
        params: MrsParams {
            rd: Reg::from(reg_rd),
            sysm: opcode.get_bits(0..8) as u8,
        },
    }
}
