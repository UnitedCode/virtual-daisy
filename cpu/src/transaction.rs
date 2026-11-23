//! Architectural rollback for a bus transaction that has not completed.
use super::{
    Control, ExceptionState, Fault, FaultContext, FaultStatusContext, HashMap, PSR, Processor,
    ProcessorMode,
};

macro_rules! checkpoint_fields {
    ($($(#[$attr:meta])* $field:ident: $type:ty),* $(,)?) => {
        pub(crate) struct Checkpoint { $($(#[$attr])* $field: $type),* }
        impl Processor {
            #[allow(clippy::clone_on_copy)]
            pub(crate) fn checkpoint(&self) -> Checkpoint {
                Checkpoint { $($(#[$attr])* $field: self.$field.clone()),* }
            }
            pub(crate) fn restore_checkpoint(&mut self, saved: Checkpoint) {
                $($(#[$attr])* { self.$field = saved.$field; })*
            }
        }
    };
}
checkpoint_fields! {
    r0_12: [u32;13], fp_regs: [u32;32], msp: u32, psp: u32, lr: u32, pc: u32,
    cycle_count: u64, instruction_count: u64, lifted_instructions: u64,
    exclusive_monitor: Option<(u32,usize)>, psr: PSR, primask: bool,
    #[cfg(not(feature="armv6m"))] faultmask: bool,
    #[cfg(not(feature="armv6m"))] basepri: u8,
    control: Control, mode: ProcessorMode, running: bool, sleeping: bool, exit_code: u32,
    waiting_for_event: bool, event_register: bool,
    exceptions: HashMap<usize,ExceptionState>, pending_exception_count: u32,
    pending_exceptions: Vec<usize>, pending_choice: Option<ExceptionState>,
    execution_priority: i16, itstate: u8,
    icsr: u32, vtor: u32, aircr: u32, scr: u32, ccr: u32, shcsr: u32,
    cfsr: u32, hfsr: u32, dfsr: u32, demcr: u32, mmfar: u32, bfar: u32, afsr: u32,
    #[cfg(feature="has-fp")] cpacr: u32,
    #[cfg(feature="has-fp")] fpccr: u32,
    #[cfg(feature="has-fp")] fpcar: u32,
    #[cfg(feature="has-fp")] fpdscr: u32,
    fpscr: u32, nvic_interrupt_enabled: [u32;16], nvic_interrupt_pending: [u32;16],
    dwt_ctrl: u32, dwt_cyccnt: u32, syst_rvr: u32, syst_cvr: u32, syst_csr: u32, syst_fraction: u32,
    pending_fault_trap: Option<FaultContext>, pending_fault_status: Option<FaultStatusContext>,
    last_pc: u32, device_boundary: std::cell::Cell<bool>,
}
impl Processor {
    /// Read a core register through a privileged debugger access.
    pub fn debug_read32(&mut self, address: u32) -> Result<u32, Fault> {
        let mode = self.mode;
        self.mode = ProcessorMode::HandlerMode;
        let result = crate::bus::Bus::read32(self, address);
        self.mode = mode;
        result
    }
    /// Write a core register through a privileged debugger access.
    pub fn debug_write32(&mut self, address: u32, value: u32) -> Result<(), Fault> {
        let mode = self.mode;
        self.mode = ProcessorMode::HandlerMode;
        let result = crate::bus::Bus::write32(self, address, value);
        self.mode = mode;
        result
    }
    pub(crate) fn validate_bus_access(
        &self,
        address: u32,
        width: usize,
        kind: u8,
    ) -> Result<(), Fault> {
        self.device.as_ref().map_or(Ok(()), |bus| {
            bus.validate_access(
                address,
                width,
                kind,
                self.mode != ProcessorMode::ThreadMode || !self.control.n_priv,
                self.execution_priority < 0,
            )
        })
    }
    pub(crate) fn transactional_bus(&self) -> bool {
        self.device.as_ref().is_some_and(|bus| bus.transactional())
    }
    pub(crate) fn bus_stalled(&self) -> bool {
        self.device
            .as_ref()
            .is_some_and(|bus| bus.instruction_stalled())
    }
    /// Advance clocks during a bus wait without retiring another instruction.
    pub fn advance_wait_cycles(&mut self, cycles: u32) {
        self.retire_decoded(cycles);
    }
    pub(crate) fn device_exception_phase(
        &self,
        phase: u8,
        address: u32,
        bytes: u32,
    ) -> Result<(), Fault> {
        self.device
            .as_ref()
            .map_or(Ok(()), |bus| bus.exception_phase(phase, address, bytes))
    }
}
