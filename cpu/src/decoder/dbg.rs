use crate::core::instruction::Instruction;

#[allow(non_snake_case)]
pub fn decode_DBG_t1(opcode: u32) -> Instruction {
    if cfg!(any(feature = "armv7m", feature = "armv7em")) {
        Instruction::NOP { thumb32: true }
    } else {
        super::decode_UDF_t2(opcode)
    }
}
