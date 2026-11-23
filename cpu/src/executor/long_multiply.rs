//! DSP long accumulates and shared architectural register validation.
use crate::{
    Processor,
    core::{
        fault::Fault,
        instruction::Reg643232Params,
        register::{BaseReg, Reg},
    },
    executor::{ExecuteResult, ExecuteSuccess, ExecutorHelper},
};
pub(super) fn validate_long_registers(params: &Reg643232Params) -> Result<(), Fault> {
    if params.rdlo == params.rdhi
        || [params.rdlo, params.rdhi, params.rn, params.rm]
            .iter()
            .any(|r| matches!(r, Reg::SP | Reg::PC))
    {
        Err(Fault::UndefInstr)
    } else {
        Ok(())
    }
}
impl Processor {
    pub(super) fn exec_dsp_long_multiply(&mut self, opcode: u32) -> ExecuteResult {
        if !self.condition_passed() {
            return Ok(ExecuteSuccess::NotTaken);
        }
        if !cfg!(feature = "has-dsp-ext") {
            return Err(Fault::UndefInstr);
        }
        let halfword = opcode & 0xfff000c0 == 0xfbc00080;
        let add_dual = opcode & 0xfff000e0 == 0xfbc000c0;
        let sub_dual = opcode & 0xfff000e0 == 0xfbd000c0;
        let unsigned_accumulate = opcode & 0xfff000f0 == 0xfbe00060;
        if !(halfword || add_dual || sub_dual || unsigned_accumulate) {
            return Err(Fault::UndefInstr);
        }
        let params = Reg643232Params {
            rdlo: Reg::from(((opcode >> 12) & 15) as u8),
            rdhi: Reg::from(((opcode >> 8) & 15) as u8),
            rn: Reg::from(((opcode >> 16) & 15) as u8),
            rm: Reg::from((opcode & 15) as u8),
        };
        validate_long_registers(&params)?;
        let a = self.get_r(params.rn);
        let b = self.get_r(params.rm);
        let lo = self.get_r(params.rdlo);
        let hi = self.get_r(params.rdhi);
        let result = if unsigned_accumulate {
            u64::from(a) * u64::from(b) + u64::from(lo) + u64::from(hi)
        } else {
            let half = |v: u32| i64::from(v as i16);
            let product = if halfword {
                half(a >> (((opcode >> 5) & 1) * 16)) * half(b >> (((opcode >> 4) & 1) * 16))
            } else {
                let b = if opcode & 16 != 0 {
                    b.rotate_right(16)
                } else {
                    b
                };
                let low = half(a) * half(b);
                let high = half(a >> 16) * half(b >> 16);
                if sub_dual { low - high } else { low + high }
            };
            ((u64::from(hi) << 32) | u64::from(lo)).wrapping_add(product as u64)
        };
        self.set_r(params.rdlo, result as u32);
        self.set_r(params.rdhi, (result >> 32) as u32);
        Ok(ExecuteSuccess::Taken { cycles: 1 })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::{instruction::Instruction, thumb::ThumbCode},
        decoder::Decoder,
    };
    #[test]
    fn invalid_long_registers_fault_before_writing() {
        let bases = [
            0xfb810302, 0xfba10302, 0xfbc10302, 0xfbe10302, 0xfbc10382, 0xfbc10392, 0xfbc103a2,
            0xfbc103b2, 0xfbc103c2, 0xfbc103d2, 0xfbd103c2, 0xfbd103d2, 0xfbe10362,
        ];
        for (index, base) in bases.into_iter().enumerate() {
            if index >= 4 && !cfg!(feature = "has-dsp-ext") {
                continue;
            }
            for bad in [13u32, 15] {
                for shift in [0, 8, 12, 16] {
                    let mut cpu = Processor::new();
                    cpu.r0_12.fill(0xdeadbeef);
                    let opcode = (base & !(15 << shift)) | (bad << shift);
                    let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                    assert_eq!(cpu.execute_internal(&instruction), Err(Fault::UndefInstr));
                    assert_eq!(cpu.r0_12, [0xdeadbeef; 13]);
                }
            }
            let mut cpu = Processor::new();
            let opcode = base & !0x00000f00;
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            assert_eq!(cpu.execute_internal(&instruction), Err(Fault::UndefInstr));
        }
        if !cfg!(feature = "has-dsp-ext") {
            let mut cpu = Processor::new();
            assert_eq!(
                cpu.execute_internal(&Instruction::DSP { opcode: 0xfbe10362 }),
                Err(Fault::UndefInstr)
            );
        }
    }
}
