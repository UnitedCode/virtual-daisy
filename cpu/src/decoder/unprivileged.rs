use crate::core::instruction::Instruction;
#[allow(non_snake_case)]
pub fn decode_UNPRIV_t1(opcode: u32) -> Instruction {
    if cfg!(any(feature = "armv7m", feature = "armv7em")) {
        Instruction::Unprivileged { opcode }
    } else {
        Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        }
    }
}
