// Modified for Virtual Daisy; see UPSTREAM.md.
use crate::core::{
    instruction::{Instruction, VAddSubParamsf32, VAddSubParamsf64},
    register::{DoubleReg, SingleReg},
};

#[allow(non_snake_case)]
pub fn decode_VMINMAXNM_t1(opcode: u32) -> Instruction {
    let d = ((opcode >> 22) & 1) as u8;
    let n = ((opcode >> 7) & 1) as u8;
    let m = ((opcode >> 5) & 1) as u8;
    let vd = ((opcode >> 12) & 15) as u8;
    let vn = ((opcode >> 16) & 15) as u8;
    let vm = (opcode & 15) as u8;
    let maximum = opcode & (1 << 6) == 0;
    if opcode & (1 << 8) == 0 {
        Instruction::VMINMAXNM_f32 {
            params: VAddSubParamsf32 {
                sd: SingleReg::from((vd << 1) | d),
                sn: SingleReg::from((vn << 1) | n),
                sm: SingleReg::from((vm << 1) | m),
            },
            maximum,
        }
    } else if !cfg!(feature = "fpv5-d16") || d | n | m != 0 {
        Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        }
    } else {
        Instruction::VMINMAXNM_f64 {
            params: VAddSubParamsf64 {
                dd: DoubleReg::from(vd),
                dn: DoubleReg::from(vn),
                dm: DoubleReg::from(vm),
            },
            maximum,
        }
    }
}
