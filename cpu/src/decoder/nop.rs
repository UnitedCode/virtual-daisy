use crate::core::instruction::Instruction;

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_NOP_t1(_: u16) -> Instruction {
    Instruction::NOP { thumb32: false }
}

#[allow(non_snake_case)]
pub fn decode_NOP_t2(_opcode: u32) -> Instruction {
    Instruction::NOP { thumb32: true }
}

#[allow(non_snake_case)]
pub fn decode_HINT_t2(opcode: u32) -> Instruction {
    if opcode & 0x700 == 0 {
        Instruction::NOP { thumb32: true }
    } else {
        Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        }
    }
}
