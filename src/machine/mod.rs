//! Rust execution of unchanged Daisy Seed firmware.
//!
//! The machine owns the CPU and physical board. Carrier hardware (keypads,
//! displays and controls) belongs to the caller and attaches through `Carrier`.
mod image;
mod seed;
mod serial;
mod timer;
mod wire;
use crate::board::Board;
pub use seed::{Carrier, Seed};
use std::{cell::RefCell, fmt, rc::Rc};
use zmu_cortex_m::{
    core::{
        exception::{Exception, ExceptionHandling, ExceptionState},
        fault::FaultTrapMode,
        register::{BaseReg, Reg},
        reset::Reset,
    },
    executor::Executor,
    memory::ram::RAM,
    peripheral::nvic::NVIC,
    Processor,
};

/// A firmware-load, unsupported-device, or execution error.
#[derive(Debug, Clone)]
pub struct Error(String);
impl Error {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// A CPU and Seed board with no instrument-specific assumptions.
pub struct Machine {
    cpu: Processor,
    seed: Rc<RefCell<Seed>>,
    irq_levels: [u32; 5],
    steps: u64,
    vectors: u32,
    resets: u64,
}
impl Machine {
    /// Load an ARM ELF executable without rebuilding or patching it.
    pub fn from_elf(board: Board, bytes: &[u8]) -> Result<Self, Error> {
        Self::from_elf_at(board, bytes, 0x08000000)
    }
    /// Load firmware with a relocated vector table (for example an ITCM probe).
    pub fn from_elf_at(board: Board, bytes: &[u8], vectors: u32) -> Result<Self, Error> {
        let mut seed = Seed::new(board)?;
        image::load(&mut seed, bytes)?;
        Self::boot(seed, vectors)
    }
    /// Load a raw flash image at 0x08000000.
    pub fn from_binary(board: Board, bytes: &[u8]) -> Result<Self, Error> {
        if !(8..=128 * 1024).contains(&bytes.len()) {
            return Err(Error::new("raw firmware must contain 8..131072 bytes"));
        }
        let mut seed = Seed::new(board)?;
        seed.load_bytes(0x08000000, bytes)?;
        Self::boot(seed, 0x08000000)
    }
    fn boot(mut seed: Seed, vectors: u32) -> Result<Self, Error> {
        if vectors & 127 != 0 || !Seed::loadable(vectors, 8) {
            return Err(Error::new(
                "vector table must be 128-byte aligned in populated memory",
            ));
        }
        let sp = seed.host_read(vectors, 4)?;
        let entry = seed.host_read(vectors + 4, 4)?;
        if sp & 3 != 0
            || sp < 4
            || !Seed::loadable(sp - 4, 4)
            || entry & 1 == 0
            || !Seed::loadable(entry & !1, 2)
        {
            return Err(Error::new(
                "invalid initial stack pointer or Thumb reset vector",
            ));
        }
        let seed = Rc::new(RefCell::new(seed));
        let cpu = Self::processor(&seed, vectors, 0)?;
        Ok(Self {
            cpu,
            seed,
            irq_levels: [0; 5],
            steps: 0,
            vectors,
            resets: 0,
        })
    }
    fn processor(seed: &Rc<RefCell<Seed>>, vectors: u32, cycles: u64) -> Result<Processor, Error> {
        let mut cpu = Processor::new();
        cpu.flash_memory(0, &[]);
        cpu.sram = RAM::new(0, 0);
        cpu.device(Some(Box::new(seed::Device(seed.clone()))));
        cpu.cpuid = 0x411fc271;
        cpu.ccr = 1 << 9;
        cpu.ictr = 4;
        cpu.priority_bits = 4;
        cpu.vtor = vectors;
        cpu.cycle_count = cycles;
        for n in 0..160 {
            let irq = Exception::Interrupt { n };
            cpu.exceptions
                .entry(irq.into())
                .or_insert_with(|| ExceptionState::new(irq, 0));
        }
        cpu.fault_trap_mode(FaultTrapMode::none());
        cpu.reset().map_err(|e| Error::new(e.to_string()))?;
        cpu.lr = u32::MAX;
        Ok(cpu)
    }
    /// Attach a project's external controls and I²C devices before running it.
    pub fn attach(&mut self, carrier: Box<dyn Carrier>) {
        self.seed.borrow_mut().carrier = Some(carrier);
    }
    fn service(&mut self) -> Result<(), Error> {
        let mut seed = self.seed.borrow_mut();
        seed.synchronize(self.cpu.cycle_count)?;
        if let Some(message) = seed.error.as_ref() {
            return Err(Error::new(format!(
                "{message}; PC=0x{:08x}",
                self.cpu.get_pc()
            )));
        }
        for (n, asserted) in seed.interrupts() {
            let bank = &mut self.irq_levels[n / 32];
            let mask = 1 << (n % 32);
            let rising = *bank & mask == 0;
            if asserted {
                *bank |= mask;
                if rising || !self.cpu.exception_active(Exception::Interrupt { n }) {
                    self.cpu.nvic_write_ispr(n / 32, mask);
                }
            } else {
                *bank &= !mask;
            }
        }
        Ok(())
    }
    /// Execute one instruction, exception entry, or scheduled bus-wait interval.
    pub fn step(&mut self) -> Result<(), Error> {
        self.step_limit(u64::MAX)
    }
    fn step_limit(&mut self, maximum_ns: u64) -> Result<(), Error> {
        self.steps += 1;
        self.service()?;
        if self.seed.borrow().reset_requested {
            self.seed.borrow_mut().reset_board()?;
            self.cpu = Self::processor(&self.seed, self.vectors, self.cpu.cycle_count)?;
            self.irq_levels = [0; 5];
            self.resets += 1;
            return Ok(());
        }
        if self.cpu.sleeping
            && self.cpu.get_pending_exception().is_none()
            && !self.cpu.has_wakeup_condition()
        {
            let cycles = self.seed.borrow().next_cycles(maximum_ns)?;
            self.cpu.advance_wait_cycles(
                cycles.min(self.cpu.next_systick_cycles().unwrap_or(u32::MAX)),
            );
        } else {
            if self.cpu.sleeping && self.cpu.has_wakeup_condition() {
                self.cpu.sleeping = false;
            }
            self.cpu.step();
        }
        if let Some(fault) = self.cpu.take_pending_fault_trap() {
            return Err(Error::new(format!("{fault}; CFSR=0x{:08x}", self.cpu.cfsr)));
        }
        if self.seed.borrow().waiting {
            let cycles = self
                .seed
                .borrow()
                .next_cycles(self.seed.borrow().wait_ns.max(1).min(maximum_ns))?;
            self.cpu.advance_wait_cycles(
                cycles.min(self.cpu.next_systick_cycles().unwrap_or(u32::MAX)),
            );
        }
        self.service()
    }
    /// Advance at most `maximum_steps`, stopping at the requested virtual time.
    /// A finite step budget prevents malformed firmware from hanging its caller.
    pub fn run_until(&mut self, nanoseconds: u64, maximum_steps: u64) -> Result<(), Error> {
        for _ in 0..maximum_steps {
            if self.time_ns() >= nanoseconds {
                return Ok(());
            }
            self.step_limit(nanoseconds - self.time_ns())?;
        }
        if self.time_ns() >= nanoseconds {
            return Ok(());
        }
        Err(Error::new(format!(
            "step budget exhausted at {} ns (target {nanoseconds} ns)",
            self.time_ns()
        )))
    }
    /// Elapsed board time in nanoseconds.
    pub fn time_ns(&self) -> u64 {
        self.seed.borrow().now
    }
    /// Total scheduler steps, including waits and exception entry.
    pub fn steps(&self) -> u64 {
        self.steps
    }
    /// Number of keyed SYSRESETREQ requests completed by the board host.
    pub fn resets(&self) -> u64 {
        self.resets
    }
    /// Assert one external interrupt pending bit, including while NVIC-disabled.
    pub fn pend_interrupt(&mut self, interrupt: usize) -> Result<(), Error> {
        if interrupt >= 160 {
            return Err(Error::new("external interrupt outside 0..159"));
        }
        self.cpu
            .nvic_write_ispr(interrupt / 32, 1 << (interrupt % 32));
        Ok(())
    }
    /// Issue one peripheral DMA request edge for a fixture or attached device.
    pub fn request_dma(&mut self, stream: usize) -> Result<(), Error> {
        self.seed
            .borrow_mut()
            .request_dma(stream)
            .map_err(Error::from)
    }
    /// Inject explicit competing-master traffic for a conformance scenario.
    /// Target numbers are documented in the bus model; this is not measured silicon timing.
    pub fn inject_contention(
        &mut self,
        target: usize,
        nanoseconds: u64,
        write: bool,
    ) -> Result<(), Error> {
        self.seed
            .borrow_mut()
            .contention(target, nanoseconds, write)
            .map_err(Error::from)
    }
    /// Cache, memory-controller and fabric counters for a captured run.
    pub fn memory_report(&self) -> String {
        self.seed.borrow().memory_report()
    }
    /// Exact codec frame rate once firmware has enabled audio clocks.
    pub fn sample_rate(&self) -> Option<crate::clock::Rate> {
        self.seed.borrow().sample_rate()
    }
    /// Snapshot the external flash chip, including completed firmware writes.
    pub fn nonvolatile_image(&self) -> Vec<u8> {
        self.seed.borrow().nonvolatile_image()
    }
    /// Monotonic revision of the external flash contents.
    pub fn nonvolatile_revision(&self) -> u64 {
        self.seed.borrow().nonvolatile_revision()
    }
    /// Restore a versioned `VDNOR001` flash snapshot before execution.
    pub fn restore_nonvolatile_image(&mut self, image: &[u8]) -> Result<(), Error> {
        if self.steps != 0 {
            return Err(Error::new(
                "restore external flash before executing firmware",
            ));
        }
        self.seed
            .borrow_mut()
            .restore_nonvolatile_image(image)
            .map_err(Error::from)
    }
    /// Physical memory or register read, matching a debugger rather than CPU cache.
    pub fn read32(&mut self, address: u32) -> Result<u32, Error> {
        if self.seed.borrow().contains(address) {
            self.seed
                .borrow_mut()
                .host_read(address, 4)
                .map_err(Error::from)
        } else {
            self.cpu
                .debug_read32(address)
                .map_err(|e| Error::new(e.to_string()))
        }
    }
    /// Write a board register, physical memory, or core control register.
    pub fn write32(&mut self, address: u32, value: u32) -> Result<(), Error> {
        if self.seed.borrow().contains(address) {
            self.seed
                .borrow_mut()
                .host_write(address, 4, value)
                .map_err(Error::from)
        } else {
            self.cpu
                .debug_write32(address, value)
                .map_err(|e| Error::new(e.to_string()))
        }
    }
    /// Queue stereo serial input samples. No host microphone is opened.
    pub fn input(&mut self, samples: &[[f32; 2]]) -> Result<(), Error> {
        if samples
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1.)
        {
            return Err(Error::new(
                "input samples must be finite and within full scale",
            ));
        }
        if self.seed.borrow().input.len().saturating_add(samples.len()) > 1_000_000 {
            return Err(Error::new(
                "serial input queue exceeds one million frames; feed it in smaller batches",
            ));
        }
        self.seed.borrow_mut().input.extend(samples.iter().copied());
        Ok(())
    }
    /// Number of input frames queued for the next serial audio clocks.
    pub fn queued_input_frames(&self) -> usize {
        self.seed.borrow().input.len()
    }
    /// Drain both codec output channels.
    pub fn take_audio(&mut self) -> Vec<[f32; 2]> {
        self.seed.borrow_mut().take_audio()
    }
    /// Queue incoming UART bytes at the configured baud rate.
    pub fn receive_serial(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.seed
            .borrow_mut()
            .receive_serial(bytes)
            .map_err(Error::from)
    }
    /// Drain transmitted bytes with their completed-frame timestamps.
    pub fn take_serial(&mut self) -> Vec<(u64, u8)> {
        self.seed.borrow_mut().take_serial()
    }
    /// Drive one external GPIO contact. Ports A–K are numbered 0–10.
    pub fn drive_pin(&mut self, port: u8, pin: u8, high: bool) -> Result<(), Error> {
        self.seed
            .borrow_mut()
            .drive_pin(port, pin, high)
            .map_err(Error::from)
    }
    /// Current CPU registers, useful when comparing firmware probes.
    pub fn registers(&self) -> [u32; 16] {
        let mut r = [0; 16];
        r[..13].copy_from_slice(&self.cpu.r0_12);
        r[13] = self.cpu.get_r(Reg::SP);
        r[14] = self.cpu.lr;
        r[15] = self.cpu.get_r(Reg::PC).wrapping_sub(4);
        r
    }
}
