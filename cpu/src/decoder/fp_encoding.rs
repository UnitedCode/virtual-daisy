//! Check raw register-bank bits before their enum conversions discard them.
use crate::core::{instruction::Instruction, register::ExtensionReg};

pub(super) fn valid(opcode: u32, instruction: &Instruction) -> bool {
    use Instruction::*;
    // D, N and M are the high register-number bits for double operands.
    // The populated D0–D15 bank has none of these bits set. A single operand
    // uses its corresponding bit as the low bit of S0–S31 and must keep it.
    let (mask, arithmetic) = match instruction {
        VADD_f64 { .. }
        | VSUB_f64 { .. }
        | VMUL_f64 { .. }
        | VNMUL_f64 { .. }
        | VDIV_f64 { .. }
        | VFMA_f64 { .. }
        | VFMS_f64 { .. }
        | VFNMS_f64 { .. }
        | VFNMA_f64 { .. }
        | VMLA_f64 { .. }
        | VMINMAXNM_f64 { .. }
        | VSEL_f64 { .. } => (0x0040_00a0, true),
        VABS_f64 { .. }
        | VNEG_f64 { .. }
        | VSQRT_f64 { .. }
        | VRINT_f64 { .. }
        | VRINTZ_f64 { .. } => (0x0040_0020, true),
        VCMP_f64 { params } => (0x0040_0000 | if params.with_zero { 0 } else { 0x20 }, true),
        VCVT_f32_f64 { .. } => (0x20, true),
        VCVT_f64_f32 { .. } => (0x0040_0000, true),
        VCVT_half { params } if matches!(params.full, ExtensionReg::Double { .. }) => {
            (if params.to_half { 0x20 } else { 0x0040_0000 }, true)
        }
        VCVT_fixed { params } if matches!(params.register, ExtensionReg::Double { .. }) => {
            (0x0040_0000, true)
        }
        VCVT { params } | VCVT_directed { params, .. } if params.dp_operation => {
            (if params.to_integer { 0x20 } else { 0x0040_0000 }, true)
        }
        VMOV_reg_f64 { .. } => (0x0040_0020, false),
        VMOV_imm_64 { .. } => (0x0040_0000, true),
        VMOV_cr2_dp { .. } => (0x20, false),
        VLDR { params } | VSTR { params } if matches!(params.dd, ExtensionReg::Double { .. }) => {
            (0x0040_0000, false)
        }
        _ => return true,
    };
    opcode & mask == 0 && (!arithmetic || cfg!(feature = "fpv5-d16"))
}
