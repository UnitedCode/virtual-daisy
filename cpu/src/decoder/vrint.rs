use crate::core::{
    bits::Bits,
    fpregister::IntegralRounding,
    instruction::{Instruction, VMovRegParamsf32, VMovRegParamsf64},
    register::{DoubleReg, SingleReg},
};

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_VRINTZ_t1(opcode: u32) -> Instruction {
    let sz = opcode.get_bit(8);

    let d = u8::from(opcode.get_bit(22));
    let vd = opcode.get_bits(12..16) as u8;
    let vm = opcode.get_bits(0..4) as u8;
    let m = u8::from(opcode.get_bit(5));

    if sz && (!cfg!(feature = "fpv5-d16") || d | m != 0) {
        return Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        };
    }
    if sz {
        Instruction::VRINTZ_f64 {
            params: VMovRegParamsf64 {
                dd: DoubleReg::from((d << 4) | vd),
                dm: DoubleReg::from((m << 4) | vm),
            },
        }
    } else {
        Instruction::VRINTZ_f32 {
            params: VMovRegParamsf32 {
                sd: SingleReg::from((vd << 1) | d),
                sm: SingleReg::from((vm << 1) | m),
            },
        }
    }
}

fn decode_round(opcode: u32, rounding: Option<IntegralRounding>, exact: bool) -> Instruction {
    let d = u8::from(opcode.get_bit(22));
    let m = u8::from(opcode.get_bit(5));
    let vd = opcode.get_bits(12..16) as u8;
    let vm = opcode.get_bits(0..4) as u8;
    if opcode.get_bit(8) {
        if !cfg!(feature = "fpv5-d16") || d | m != 0 {
            return Instruction::UDF {
                imm32: 0,
                opcode: opcode.into(),
                thumb32: true,
            };
        }
        Instruction::VRINT_f64 {
            params: VMovRegParamsf64 {
                dd: DoubleReg::from(vd),
                dm: DoubleReg::from(vm),
            },
            rounding,
            exact,
        }
    } else {
        Instruction::VRINT_f32 {
            params: VMovRegParamsf32 {
                sd: SingleReg::from(vd << 1 | d),
                sm: SingleReg::from(vm << 1 | m),
            },
            rounding,
            exact,
        }
    }
}

#[allow(non_snake_case)]
pub fn decode_VRINT_directed_t1(opcode: u32) -> Instruction {
    let rounding = match opcode.get_bits(16..18) {
        0 => IntegralRounding::NearestAway,
        1 => IntegralRounding::NearestEven,
        2 => IntegralRounding::PlusInfinity,
        _ => IntegralRounding::MinusInfinity,
    };
    decode_round(opcode, Some(rounding), false)
}

#[allow(non_snake_case)]
pub fn decode_VRINT_current_t1(opcode: u32) -> Instruction {
    decode_round(opcode, None, opcode.get_bit(16))
}
