// Modified for Virtual Daisy's browser backend; see UPSTREAM.md.
use crate::core::bits::Bits;
use crate::core::instruction::{Instruction, Reg3ShiftParams, SetFlags};
use crate::core::operation::decode_imm_shift;

#[allow(non_snake_case)]
pub fn decode_ORN_reg_t1(opcode: u32) -> Instruction {
    let imm3: u8 = opcode.get_bits(12..15) as u8;
    let imm2: u8 = opcode.get_bits(6..8) as u8;
    let type_: u8 = opcode.get_bits(4..6) as u8;

    let (shift_t, shift_n) = decode_imm_shift(type_, (imm3 << 2) + imm2);
    let s = opcode.get_bit(20);

    Instruction::ORN_reg {
        params: Reg3ShiftParams {
            rd: opcode.get_bits(8..12).into(),
            rn: opcode.get_bits(16..20).into(),
            rm: opcode.get_bits(0..4).into(),
            setflags: if s { SetFlags::True } else { SetFlags::False },
            shift_t,
            shift_n,
        },
    }
}

#[allow(non_snake_case)]
pub fn decode_ORN_imm_t1(opcode: u32) -> Instruction {
    match super::orr::decode_ORR_imm_t1(opcode) {
        Instruction::ORR_imm { params } => Instruction::ORN_imm { params },
        _ => unreachable!(),
    }
}
