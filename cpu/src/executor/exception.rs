// Modified for Virtual Daisy; see UPSTREAM.md.
use crate::Processor;

use crate::{
    core::exception::{Exception, ExceptionHandling},
    core::fault::Fault,
    core::register::{BaseReg, Reg},
    executor::{ExecuteSuccess, ExecutorHelper},
    semihosting::{decode_semihostcmd, semihost_return},
};

use super::ExecuteResult;

/// Branching operations
pub trait IsaException {
    fn exec_svc(&mut self) -> ExecuteResult;
    fn exec_bkpt(&mut self, imm32: u32) -> ExecuteResult;
}

impl IsaException for Processor {
    fn exec_svc(&mut self) -> ExecuteResult {
        if self.condition_passed() {
            let pc = self.get_pc();
            let return_address = pc.wrapping_add(2);
            // SVC completes before entering the handler. The saved IT state
            // must describe the instruction at the return address.
            self.it_advance();
            if !self.configurable_exception_can_preempt(Exception::SVCall) {
                let cycles =
                    self.handle_fault_with_return_address(Fault::Forced, pc, return_address);
                return Ok(ExecuteSuccess::Branched { cycles });
            }
            self.exception_entry(Exception::SVCall, return_address)?;
            return Ok(ExecuteSuccess::Branched { cycles: 12 });
        }
        Ok(ExecuteSuccess::NotTaken)
    }

    fn exec_bkpt(&mut self, imm32: u32) -> ExecuteResult {
        if imm32 == 0xab && self.semihost_func.is_some() {
            let r0 = self.get_r(Reg::R0);
            let r1 = self.get_r(Reg::R1);
            let semihost_cmd = decode_semihostcmd(r0, r1, self)?;

            if let Some(sh_func) = &mut self.semihost_func {
                let semihost_response = (sh_func)(&semihost_cmd);
                semihost_return(self, &semihost_response);
            }
            return Ok(ExecuteSuccess::Taken { cycles: 1 });
        }
        // No debugger has intercepted the breakpoint. Its immediate has no
        // architectural meaning, including the conventional semihosting 0xab.
        Err(Fault::DebugEvt)
    }
}
