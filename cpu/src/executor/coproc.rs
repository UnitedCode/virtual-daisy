// Modified for Virtual Daisy; see UPSTREAM.md.
#[cfg(feature = "has-fp")]
use super::fp_generic::FloatingPointChecks;
use crate::{
    Processor,
    core::fault::Fault,
    executor::{ExecuteSuccess, ExecutorHelper},
};

use super::ExecuteResult;

/// Branching operations
pub trait IsaCoprocessor {
    fn exec_unsupported_coprocessor(&mut self, coproc: u8, unconditional: bool) -> ExecuteResult;
}

impl IsaCoprocessor for Processor {
    fn exec_unsupported_coprocessor(&mut self, coproc: u8, unconditional: bool) -> ExecuteResult {
        if !unconditional && !self.condition_passed() {
            return Ok(ExecuteSuccess::NotTaken);
        }
        #[cfg(feature = "has-fp")]
        if matches!(coproc, 10 | 11) {
            self.check_vfp_enabled()?;
            return Err(Fault::UndefInstr);
        }
        #[cfg(not(feature = "has-fp"))]
        let _ = coproc;
        Err(Fault::Nocp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_coprocessors_fault_instead_of_silently_succeeding() {
        let mut cpu = Processor::new();
        for coproc in [0, 10, 11, 12, 15] {
            assert_eq!(
                cpu.exec_unsupported_coprocessor(coproc, true),
                Err(Fault::Nocp)
            );
        }
    }
    #[cfg(feature = "has-fp")]
    #[test]
    fn unrecognized_operations_on_an_enabled_fpu_are_undefined() {
        let mut cpu = Processor::new();
        cpu.cpacr = 0x00f0_0000;
        for coproc in [10, 11] {
            assert_eq!(
                cpu.exec_unsupported_coprocessor(coproc, true),
                Err(Fault::UndefInstr)
            );
        }
    }
    #[test]
    fn conditional_coprocessor_instructions_respect_the_it_predicate() {
        let mut cpu = Processor::new();
        cpu.set_itstate(0x08);
        assert_eq!(
            cpu.exec_unsupported_coprocessor(12, false),
            Ok(ExecuteSuccess::NotTaken)
        );
        assert_eq!(cpu.exec_unsupported_coprocessor(12, true), Err(Fault::Nocp));
    }
}
