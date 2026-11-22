//! Common single-precision instructions after CPACR/lazy-context setup.
//! This bypasses interpreter call layers, never guest DSP functions.
use crate::{
    Processor,
    core::{
        instruction::Instruction,
        register::{BaseReg, ExtensionRegOperations},
    },
};
impl Processor {
    #[inline(always)]
    pub(super) fn fast_fp(&mut self, i: &Instruction, size: usize) -> Option<u32> {
        if !self.native_float
            || self.get_itstate() != 0
            || self.cpacr & 0x00f00000 != 0x00f00000
            || self.fpccr & 1 != 0
            || (self.fpccr & (1 << 31) != 0 && !self.control.fpca)
        {
            return None;
        }
        let mut flags = 0;
        let (dst, value) = match i {
            Instruction::VMOV_reg_f32 { params } => (params.sd, self.get_sr(params.sm)),
            Instruction::VMOV_imm_32 { params } => (params.sd, params.imm32),
            Instruction::VSQRT_f32 { params } => {
                let (value, status) =
                    super::native_float::square_root(self.get_sr(params.sm), self.fpscr)?;
                flags = status;
                (params.sd, value)
            }
            Instruction::VADD_f32 { params }
            | Instruction::VSUB_f32 { params }
            | Instruction::VMUL_f32 { params }
            | Instruction::VDIV_f32 { params } => {
                let op = match i {
                    Instruction::VADD_f32 { .. } => 0,
                    Instruction::VSUB_f32 { .. } => 1,
                    Instruction::VMUL_f32 { .. } => 2,
                    _ => 3,
                };
                let (value, status) = super::native_float::binary(
                    op,
                    self.get_sr(params.sn),
                    self.get_sr(params.sm),
                    self.fpscr,
                )?;
                flags = status;
                (params.sd, value)
            }
            Instruction::VMLA_f32 {
                params,
                subtract,
                negate,
            } => {
                let (product, status1) = super::native_float::binary(
                    2,
                    self.get_sr(params.sn),
                    self.get_sr(params.sm),
                    self.fpscr,
                )?;
                let (value, status2) = super::native_float::binary(
                    0,
                    self.get_sr(params.sd) ^ if *negate { 1 << 31 } else { 0 },
                    product ^ if *subtract { 1 << 31 } else { 0 },
                    self.fpscr,
                )?;
                flags = status1 | status2;
                (params.sd, value)
            }
            Instruction::VABS_f32 { params } => (params.sd, self.get_sr(params.sm) & 0x7fff_ffff),
            Instruction::VNEG_f32 { params } => (params.sd, self.get_sr(params.sm) ^ (1 << 31)),
            _ => return None,
        };
        self.set_sr(dst, value);
        self.fpscr |= flags;
        self.instruction_count += 1;
        self.add_pc(size as u32);
        Some(1)
    }
}
