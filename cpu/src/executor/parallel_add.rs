#[cfg(feature = "has-dsp-ext")]
use super::ExecuteResult;
#[cfg(feature = "has-dsp-ext")]
use crate::{Processor, core::instruction::Reg3NoSetFlagsParams};
/// Typed UADD8 entry point retained for existing decoder/API users.
#[cfg(feature = "has-dsp-ext")]
pub trait IsaParallelAddSub {
    fn exec_uadd8(&mut self, params: &Reg3NoSetFlagsParams) -> ExecuteResult;
}

#[cfg(all(test, not(feature = "has-dsp-ext")))]
mod tests {
    use crate::{
        Processor,
        core::{
            fault::Fault,
            instruction::{Instruction, Reg3NoSetFlagsParams},
            register::Reg,
        },
        executor::ExecutorHelper,
    };
    #[test]
    fn constructed_packed_instructions_cannot_bypass_the_profile() {
        let mut cpu = Processor::new();
        let instruction = Instruction::UADD8 {
            params: Reg3NoSetFlagsParams {
                rd: Reg::R0,
                rn: Reg::R1,
                rm: Reg::R2,
            },
        };
        assert_eq!(cpu.execute_internal(&instruction), Err(Fault::UndefInstr));
        assert_eq!(
            cpu.execute_internal(&Instruction::DSP { opcode: 0xfa91f002 }),
            Err(Fault::UndefInstr)
        );
    }
}
#[cfg(feature = "has-dsp-ext")]
impl IsaParallelAddSub for Processor {
    fn exec_uadd8(&mut self, params: &Reg3NoSetFlagsParams) -> ExecuteResult {
        self.exec_parallel_dsp(
            0xfa80f040 | ((params.rn as u32) << 16) | ((params.rd as u32) << 8) | params.rm as u32,
        )
    }
}
