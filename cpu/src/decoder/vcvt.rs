#[cfg(any(feature = "fpv5-d16", feature = "fpv5-sp-d16"))]
use crate::core::fpregister::IntegralRounding;
use crate::core::{
    bits::Bits,
    instruction::{
        Instruction, VCVTFixedParams, VCVTHalfParams, VCVTParams, VCVTParamsF32F64,
        VCVTParamsF64F32,
    },
    register::{DoubleReg, ExtensionReg, SingleReg},
};

#[allow(non_snake_case)]
pub fn decode_VCVT_fixed_t1(opcode: u32) -> Instruction {
    let double = opcode & (1 << 8) != 0;
    let high = (opcode >> 22) & 1;
    let vd = (opcode >> 12) & 15;
    let width = if opcode & (1 << 7) != 0 { 32 } else { 16 };
    let immediate = ((opcode & 15) << 1) | ((opcode >> 5) & 1);
    if immediate > width || (double && (!cfg!(feature = "fpv5-d16") || high != 0)) {
        return Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        };
    }
    Instruction::VCVT_fixed {
        params: VCVTFixedParams {
            register: if double {
                ExtensionReg::Double {
                    reg: DoubleReg::from(vd as u8),
                }
            } else {
                ExtensionReg::Single {
                    reg: SingleReg::from((vd * 2 + high) as u8),
                }
            },
            to_fixed: opcode & (1 << 18) != 0,
            unsigned: opcode & (1 << 16) != 0,
            width: width as u8,
            fraction_bits: (width - immediate) as u8,
        },
    }
}

#[allow(non_snake_case)]
pub fn decode_VCVT_half_t1(opcode: u32) -> Instruction {
    let d = ((opcode >> 22) & 1) as u8;
    let m = ((opcode >> 5) & 1) as u8;
    let vd = ((opcode >> 12) & 15) as u8;
    let vm = (opcode & 15) as u8;
    let to_half = opcode & (1 << 16) != 0;
    let double = opcode & (1 << 8) != 0;
    if double && (!cfg!(feature = "fpv5-d16") || if to_half { m != 0 } else { d != 0 }) {
        return Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        };
    }
    let half = SingleReg::from(if to_half { vd << 1 | d } else { vm << 1 | m });
    let full = if double {
        ExtensionReg::Double {
            reg: DoubleReg::from(if to_half { vm } else { vd }),
        }
    } else {
        ExtensionReg::Single {
            reg: SingleReg::from(if to_half { vm << 1 | m } else { vd << 1 | d }),
        }
    };
    Instruction::VCVT_half {
        params: VCVTHalfParams {
            half,
            full,
            to_half,
            top: opcode & (1 << 7) != 0,
        },
    }
}

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_VCVT_t1(opcode: u32) -> Instruction {
    let vm = opcode.get_bits(0..4) as u8;
    let M = u8::from(opcode.get_bit(5));
    let op = opcode.get_bit(7);
    let sz = opcode.get_bit(8);
    let vd = opcode.get_bits(12..16) as u8;
    let opc2 = opcode.get_bits(16..19) as u8;
    let D = u8::from(opcode.get_bit(22));

    let to_integer = opc2.get_bit(2);
    let dp_operation = sz;
    if dp_operation && (!cfg!(feature = "fpv5-d16") || if to_integer { M != 0 } else { D != 0 }) {
        return Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        };
    }
    if to_integer {
        let unsigned = !opc2.get_bit(0);
        let round_zero = op;
        let d = vd << 1 | D;
        let m = if dp_operation {
            M << 4 | vm
        } else {
            vm << 1 | M
        };
        Instruction::VCVT {
            params: VCVTParams {
                to_integer,
                dp_operation,
                unsigned,
                round_zero,
                round_nearest: false,
                d: ExtensionReg::Single {
                    reg: SingleReg::from(d),
                },
                m: if dp_operation {
                    ExtensionReg::Double {
                        reg: DoubleReg::from(m),
                    }
                } else {
                    ExtensionReg::Single {
                        reg: SingleReg::from(m),
                    }
                },
            },
        }
    } else {
        let unsigned = !op;
        let round_nearest = false;
        let m = vm << 1 | M;
        let d = if dp_operation {
            D << 4 | vd
        } else {
            vd << 1 | D
        };
        Instruction::VCVT {
            params: VCVTParams {
                to_integer,
                dp_operation,
                unsigned,
                round_zero: false,
                round_nearest,
                d: if dp_operation {
                    ExtensionReg::Double {
                        reg: DoubleReg::from(d),
                    }
                } else {
                    ExtensionReg::Single {
                        reg: SingleReg::from(d),
                    }
                },
                m: ExtensionReg::Single {
                    reg: SingleReg::from(m),
                },
            },
        }
    }
}

#[cfg(any(feature = "fpv5-d16", feature = "fpv5-sp-d16"))]
#[allow(non_snake_case)]
pub fn decode_VCVT_directed_t1(opcode: u32) -> Instruction {
    let d = ((opcode >> 22) & 1) as u8;
    let m = ((opcode >> 5) & 1) as u8;
    let vd = ((opcode >> 12) & 15) as u8;
    let vm = (opcode & 15) as u8;
    let double = opcode & (1 << 8) != 0;
    if double && (!cfg!(feature = "fpv5-d16") || m != 0) {
        return Instruction::UDF {
            imm32: 0,
            opcode: opcode.into(),
            thumb32: true,
        };
    }
    let rounding = match (opcode >> 16) & 3 {
        0 => IntegralRounding::NearestAway,
        1 => IntegralRounding::NearestEven,
        2 => IntegralRounding::PlusInfinity,
        _ => IntegralRounding::MinusInfinity,
    };
    Instruction::VCVT_directed {
        params: VCVTParams {
            d: ExtensionReg::Single {
                reg: SingleReg::from(vd << 1 | d),
            },
            m: if double {
                ExtensionReg::Double {
                    reg: DoubleReg::from(vm),
                }
            } else {
                ExtensionReg::Single {
                    reg: SingleReg::from(vm << 1 | m),
                }
            },
            to_integer: true,
            unsigned: opcode & (1 << 7) == 0,
            dp_operation: double,
            round_zero: false,
            round_nearest: false,
        },
        rounding,
    }
}

#[allow(non_snake_case)]
#[inline(always)]
pub fn decode_VCVT_ds_t1(opcode: u32) -> Instruction {
    let vm = opcode.get_bits(0..4) as u8;
    let m = u8::from(opcode.get_bit(5));
    let sz = opcode.get_bit(8);
    let vd = opcode.get_bits(12..16) as u8;
    let d = u8::from(opcode.get_bit(22));

    if sz {
        Instruction::VCVT_f32_f64 {
            params: VCVTParamsF32F64 {
                sd: SingleReg::from((vd << 1) | d),
                dm: DoubleReg::from((m << 4) | vm),
            },
        }
    } else {
        Instruction::VCVT_f64_f32 {
            params: VCVTParamsF64F32 {
                dd: DoubleReg::from((d << 4) | vd),
                sm: SingleReg::from((vm << 1) | m),
            },
        }
    }
}
