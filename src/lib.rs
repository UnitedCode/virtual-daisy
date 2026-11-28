//! Daisy Seed board models and Rust firmware host, with an optional Renode bridge.
use std::collections::BTreeMap;
pub mod audio;
pub mod board;
pub mod bus;
pub mod clock;
mod codec;
pub use codec::SampleClocks;
mod cpu_progress;
mod dma;
mod flash;
mod gpio;
pub mod host;
pub mod keyboard;
#[cfg(feature = "rust-host")]
pub mod machine;
pub mod memory;
pub mod nor;
pub mod qspi;
mod sai;
mod sai_capture;
#[cfg(test)]
mod tests;
pub mod uart;

/// Stateless half conversion used by the optional reference CPU adapter.
/// Returns nonzero for invalid inputs or a library built without `rust-host`.
/// # Safety
/// `result` and `flags` must point to writable output values.
#[no_mangle]
pub unsafe extern "C" fn daisy_fp_half(
    bits: u64,
    precision: u32,
    to_half: u32,
    control: u32,
    result: *mut u64,
    flags: *mut u32,
) -> u32 {
    #[cfg(feature = "rust-host")]
    {
        if to_half > 1 {
            return 1;
        }
        match zmu_cortex_m::convert_half_bits(bits, precision, to_half != 0, control) {
            Some((value, status)) => {
                *result = value;
                *flags = status;
                0
            }
            None => 1,
        }
    }
    #[cfg(not(feature = "rust-host"))]
    {
        let _ = (bits, precision, to_half, control, result, flags);
        1
    }
}

/// PSR field service for the optional reference adapter.
/// The low word is the result; a nonzero high word reports unsupported input.
/// `write` is 0 for MRS or 1 for MSR; `sysm` and `mask` are encoded fields.
/// Libraries built without `rust-host` report the service as unsupported.
#[no_mangle]
pub extern "C" fn daisy_psr_bits(
    current: u32,
    input: u32,
    sysm: u32,
    mask: u32,
    write: u32,
) -> u64 {
    #[cfg(feature = "rust-host")]
    {
        if sysm > 7 || mask > 3 || write > 1 {
            return 1u64 << 32;
        }
        let result = if write == 0 {
            zmu_cortex_m::core::status::read_psr_bits(current, sysm as u8)
        } else if sysm < 4 {
            zmu_cortex_m::core::status::write_apsr_bits(current, input, mask as u8)
        } else {
            None
        };
        result.map_or(1u64 << 32, u64::from)
    }
    #[cfg(not(feature = "rust-host"))]
    {
        let _ = (current, input, sysm, mask, write);
        1u64 << 32
    }
}

pub struct Peripheral {
    kind: u32,
    registers: BTreeMap<u64, u32>,
    dma: dma::Dma,
    sai: sai::Sai,
    qspi: std::sync::Arc<std::sync::Mutex<qspi::Qspi>>,
    gpio: gpio::Gpio,
    codec_reset_high: bool,
    memory: std::sync::Mutex<memory::SystemMemory>,
    clock: clock::ClockTree,
    cpu_progress: cpu_progress::CpuProgress,
    line_stage: codec::LineStage,
    passband_delay: codec::PassbandDelay,
    axi: bus::Axi,
    fmc_commands: std::collections::VecDeque<u32>,
}

impl Peripheral {
    pub fn nonvolatile_image(&self) -> Vec<u8> {
        self.qspi.lock().unwrap().nonvolatile_image()
    }
    pub fn nonvolatile_revision(&self) -> u64 {
        self.qspi.lock().unwrap().nonvolatile_revision()
    }
    pub fn restore_nonvolatile_image(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        if self.kind != 8 {
            return Err("not a QSPI controller");
        }
        self.qspi.lock().unwrap().restore_nonvolatile_image(bytes)
    }
    /// Feed one serial audio frame without filesystem or native-bridge IO.
    pub fn audio_frame(&mut self, codec_mask: u32, sample: [f32; 2]) -> Result<(), &'static str> {
        if self.kind != 7 {
            return Err("not an SAI peripheral");
        }
        self.sai.tick_sample(codec_mask, sample);
        Ok(())
    }
    /// Clock one stereo frame through the selected codec's digital routing.
    /// `ideal_line` additionally enables the documented WM8731 line surrogate.
    pub fn audio_frame_with_codec(
        &mut self,
        codec: &mut Peripheral,
        sample: [f32; 2],
        ideal_line: bool,
    ) -> Result<(), &'static str> {
        let mask = codec.read(0x100)?;
        let input = if ideal_line {
            codec.nominal_adc_passband(sample)?
        } else {
            sample
        };
        let previous = self.sai.output.len();
        self.audio_frame(mask, input)?;
        if self.sai.output.len() != previous {
            if let Some(frame) = self.sai.output.last_mut() {
                if codec.kind == 4 && codec.registers.get(&7).copied().unwrap_or(0) & (1 << 5) != 0
                {
                    frame.swap(0, 1);
                }
                if ideal_line {
                    let delayed = codec.nominal_dac_passband(*frame)?;
                    *frame = if mask & 2 != 0 { delayed } else { [0.; 2] };
                }
            }
        }
        Ok(())
    }
    /// Drain captured frames, permitting bounded streaming in a browser.
    pub fn take_audio(&mut self) -> Result<Vec<[f32; 2]>, &'static str> {
        if self.kind != 7 {
            return Err("not an SAI peripheral");
        }
        Ok(std::mem::take(&mut self.sai.output))
    }
    /// Active SAI DMA request lines, in A/B order.
    pub fn audio_requests(&self) -> u32 {
        self.sai.requests()
    }
    /// Shared SAI A/B interrupt level, including each block's mask.
    pub fn audio_interrupt(&self) -> bool {
        self.sai.irq()
    }
    /// DMA arbitration and descriptor inspection without advancing NDTR.
    pub fn dma_transfer(
        &mut self,
        requests: u32,
    ) -> Result<Option<(usize, u32, u32)>, &'static str> {
        let Some(stream) = self.dma.arbitrate(requests) else {
            return Ok(None);
        };
        match self.dma.clone().request(stream)? {
            Some((a, b)) => Ok(Some((stream, a, b))),
            None => {
                self.dma.request(stream)?;
                Ok(None)
            }
        }
    }
    /// Complete a transferred descriptor beat and return its bus addresses.
    pub fn dma_complete(&mut self, stream: usize) -> Result<Option<(u32, u32)>, &'static str> {
        self.dma.request(stream)
    }
    /// Asserted DMA interrupt lines (stream order).
    pub fn dma_interrupts(&self) -> u32 {
        self.dma.irqs()
    }
    /// Complete a documented DMA bus error without changing the transfer count.
    pub fn dma_bus_error(&mut self, stream: usize) -> Result<(), &'static str> {
        self.dma.bus_error(stream)
    }
    /// Functional core clock after RCC selection/dividers.
    pub fn core_hz(&self) -> Result<f64, &'static str> {
        Ok(self
            .clock
            .core_rate()?
            .map_or(64_000_000.0, clock::Rate::hz))
    }
    /// Peripheral domain clock (domain 6 is the APB1 timer clock).
    pub fn domain_hz(&self, domain: u32) -> Result<f64, &'static str> {
        Ok(self.clock.domain_rate(domain)?.map_or(0.0, clock::Rate::hz))
    }
    /// Advance the shared SAI clock's exact rational phase.
    pub fn audio_clock(&mut self, cr1: u32, frcr: u32, ns: u64) -> Result<u32, &'static str> {
        self.clock.advance(cr1, frcr, ns)
    }
    /// Deadline for scheduling the next frame without polling the board.
    pub fn next_audio_ns(&self, cr1: u32, frcr: u32) -> Result<Option<u64>, &'static str> {
        self.clock.next_audio_ns(cr1, frcr)
    }
    /// Sample rate from the shared RCC model.
    pub fn audio_hz(&self, cr1: u32, frcr: u32) -> Result<f64, &'static str> {
        self.clock.sample_rate(cr1, frcr).map(clock::Rate::hz)
    }
    /// Drive a modeled GPIO pin from an external carrier control.
    pub fn drive_pin(&mut self, pin: u32, high: bool) -> Result<(), &'static str> {
        if self.kind < 100 {
            return Err("not GPIO");
        }
        self.gpio.drive(pin, high);
        Ok(())
    }
    /// Attach this QSPI controller to a shared architecture's mapped memory.
    pub fn attach_memory(&mut self, memory: &mut memory::SystemMemory) -> Result<(), &'static str> {
        if self.kind != 8 {
            return Err("not QSPI");
        }
        memory.attach_qspi(self.qspi.clone());
        Ok(())
    }
    /// Synchronize a QSPI controller against this RCC clock.
    pub fn sync_qspi(&self, qspi: &mut Peripheral, now: u64) -> Result<(), &'static str> {
        if self.kind != 13 || qspi.kind != 8 {
            return Err("not RCC/QSPI");
        }
        let mut controller = qspi.qspi.lock().unwrap();
        controller.advance_time(now)?;
        controller.configure_clock(self.clock.qspi_rate()?)
    }
    /// Width-aware accesses to the QSPI FIFO/control model.
    pub fn qspi_read(&mut self, offset: u64, width: usize) -> Result<u32, &'static str> {
        self.qspi.lock().unwrap().read(offset, width)
    }
    /// Width-aware accesses to the QSPI FIFO/control model.
    pub fn qspi_write(
        &mut self,
        offset: u64,
        width: usize,
        value: u32,
    ) -> Result<(), &'static str> {
        self.qspi.lock().unwrap().write(offset, value, width)
    }
    /// QSPI kernel clock from RCC selection and gates.
    pub fn qspi_clock(&self) -> Result<Option<clock::Rate>, &'static str> {
        self.clock.qspi_rate()
    }
    /// Clock-tree access for hosts that schedule the same models directly.
    pub fn clocks(&self) -> &clock::ClockTree {
        &self.clock
    }
    /// Advance or configure the RCC-backed debug cycle counter.
    pub fn dwt(&mut self, offset: u64, value: Option<u32>, now: u64) -> Result<u32, &'static str> {
        self.clock.dwt(offset, value, now)
    }
    /// Apply DEMCR.TRCENA at the exact current board time.
    pub fn trace_enable(&mut self, enabled: bool, now: u64) -> Result<(), &'static str> {
        self.clock.trace_enable(enabled, now)
    }
    /// Release an externally driven GPIO input.
    pub fn release_pin(&mut self, pin: u32) -> Result<(), &'static str> {
        if self.kind < 100 || pin >= 16 {
            return Err("not a GPIO input");
        }
        self.gpio.release(pin);
        Ok(())
    }
    /// Advance the QSPI service and flash-chip deadlines on the virtual clock.
    pub fn advance_qspi(
        &mut self,
        now: u64,
        rate: Option<clock::Rate>,
    ) -> Result<(), &'static str> {
        let mut q = self.qspi.lock().unwrap();
        q.advance_time(now)?;
        q.configure_clock(rate)
    }
    /// Next QSPI or flash completion on the board's absolute nanosecond clock.
    pub fn next_qspi_event(&self) -> Option<u64> {
        self.qspi.lock().unwrap().next_event()
    }
    /// QSPI interrupt level from the controller's status and interrupt mask.
    pub fn qspi_interrupt(&self) -> bool {
        self.qspi.lock().unwrap().irq()
    }
    pub fn codec_clocks(&self, mclk: clock::Rate) -> Result<SampleClocks, &'static str> {
        if self.kind != 4 {
            return Err("software sample clocks only apply to WM8731");
        }
        codec::Sampling::decode(self.registers.get(&8).copied().unwrap_or(0))?.clocks(mclk)
    }
    pub fn validate_codec_clocks(
        &mut self,
        mclk: clock::Rate,
        frame: clock::Rate,
    ) -> Result<(), &'static str> {
        if self.kind == 9 && !self.codec_reset_high {
            return Ok(());
        }
        if self.kind == 9 || self.kind == 10 {
            if mclk.numerator == 0
                || mclk.denominator == 0
                || frame.numerator == 0
                || frame.denominator == 0
            {
                return Err("hardware codec requires a valid MCLK and LRC");
            }
            if self.kind == 10
                && (u128::from(frame.numerator) < 16_000 * u128::from(frame.denominator)
                    || u128::from(frame.numerator) > 96_000 * u128::from(frame.denominator)
                    || u128::from(mclk.numerator) < 2_048_000 * u128::from(mclk.denominator)
                    || u128::from(mclk.numerator) > 36_864_000 * u128::from(mclk.denominator))
            {
                return Err("PCM3060 clocks exceed the shared ADC/DAC operating range");
            }
            // Necessary clock ratios from AK4556 Table3 and PCM3060's ADC
            // system-clock requirements. AK pin-selected speed/range behavior
            // is not inferred from the union of its documented slave modes.
            let ratios: &[u64] = if self.kind == 9 {
                &[128, 192, 256, 384, 512, 768]
            } else {
                &[256, 384, 512, 768]
            };
            if !ratios.iter().any(|&ratio| {
                u128::from(mclk.numerator) * u128::from(frame.denominator)
                    == u128::from(frame.numerator)
                        * u128::from(mclk.denominator)
                        * u128::from(ratio)
                    && (self.kind != 9 || {
                        // AK4556 Table2: union of speed modes for this ratio,
                        // not evidence that the board's CKS straps select it.
                        let (min, max, exclusive) = match ratio {
                            128 => (108_000, 216_000, true),
                            192 => (96_000, 192_000, true),
                            256 => (8_000, 108_000, false),
                            384 => (8_000, 96_000, false),
                            512 => (8_000, 54_000, false),
                            _ => (8_000, 48_000, false),
                        };
                        let hz = u128::from(frame.numerator);
                        let denominator = u128::from(frame.denominator);
                        hz <= max * denominator
                            && if exclusive {
                                hz > min * denominator
                            } else {
                                hz >= min * denominator
                            }
                    })
            }) {
                return Err("hardware codec ADC/DAC system-clock ratio is unsupported");
            }
            return Ok(());
        }
        if self.kind != 4 {
            return Err("not a Seed codec");
        }
        if self.registers.get(&9).copied().unwrap_or(0) & 1 == 0 {
            return Ok(());
        }
        self.read(0x100)?;
        let power = self.registers.get(&6).copied().unwrap_or(0);
        if power & 0x80 != 0 {
            return Ok(());
        }
        let clocks = self.codec_clocks(mclk)?;
        let same = |rate: clock::Rate| {
            u128::from(rate.numerator) * u128::from(frame.denominator)
                == u128::from(frame.numerator) * u128::from(rate.denominator)
        };
        if frame.numerator == 0
            || frame.denominator == 0
            || (power & 4 == 0 && !same(clocks.adc))
            || (power & 8 == 0 && !same(clocks.dac))
        {
            return Err("WM8731 ADC/DAC sample rate does not match the shared SAI LRC");
        }
        Ok(())
    }
    pub fn codec_report(&self, ideal_line: bool, rate: f64) -> Result<String, &'static str> {
        let (name, adc, dac) = match self.kind {
            4 => {
                let (adc, dac) =
                    codec::Sampling::decode(self.registers.get(&8).copied().unwrap_or(0))?.delays();
                ("WM8731", adc as f64, dac as f64)
            }
            9 => ("AK4556", 18.0, 21.0),
            10 => ("PCM3060", 17.4, 20.0),
            _ => return Err("not a Seed codec"),
        };
        Ok(format!("codec={name}\naudio_boundary={}\nnominal_adc_group_delay_frames={adc}\nnominal_dac_group_delay_frames={dac}\nnominal_adc_group_delay_us={}\nnominal_dac_group_delay_us={}\npassband_delay_surrogate_applied={}\ncomplete_decimation_and_interpolation_response=UNMODELED\ninternal_coefficients_and_rounding=UNVERIFIED\n",if ideal_line { "ideal-line" } else { "serial" },adc/rate*1_000_000.0,dac/rate*1_000_000.0,u32::from(ideal_line&&self.kind==4)))
    }
    pub fn nominal_adc_passband(&mut self, sample: [f32; 2]) -> Result<[f32; 2], &'static str> {
        let sample = self.ideal_line_input(sample)?;
        let (delay, _) =
            codec::Sampling::decode(self.registers.get(&8).copied().unwrap_or(0))?.delays();
        Ok(self.passband_delay.adc(sample, delay))
    }
    pub fn nominal_dac_passband(&mut self, sample: [f32; 2]) -> Result<[f32; 2], &'static str> {
        if self.kind != 4 {
            return Err("nominal passband delay only modeled for WM8731");
        }
        self.read(0x100)?;
        let (_, delay) =
            codec::Sampling::decode(self.registers.get(&8).copied().unwrap_or(0))?.delays();
        Ok(self.passband_delay.dac(sample, delay))
    }
    /// Idealized normalized line input before ADC gain/DC removal. Serial PCM
    /// injection remains a separate boundary and never runs this stage twice.
    pub fn ideal_line_input(&mut self, sample: [f32; 2]) -> Result<[f32; 2], &'static str> {
        if self.kind != 4 {
            return Err("ideal line stage only implemented for WM8731");
        }
        let mask = self.read(0x100)?;
        if mask & 1 == 0 {
            return Ok([0.0; 2]);
        }
        let reg = |index| self.registers.get(&index).copied().unwrap_or(0);
        if reg(4) & 0x0c != 0 {
            return Err("WM8731 microphone or analog bypass route is not modeled");
        }
        Ok(self.line_stage.process(sample, [reg(0), reg(1)], reg(5)))
    }
    pub fn configure_revision_straps(&mut self, grounded: u32) -> Result<(), &'static str> {
        if self.kind != 103 || grounded > 7 {
            return Err("invalid port D revision straps");
        }
        for bit in 0..3 {
            self.gpio.release(3 + bit);
            if grounded & (1 << bit) != 0 {
                self.gpio.drive(3 + bit, false);
            }
        }
        Ok(())
    }
    pub fn codec_control(&mut self, high: bool) -> Result<(), &'static str> {
        if self.kind != 9 {
            return Err("this codec has no modeled GPIO reset connection");
        }
        self.codec_reset_high = high;
        Ok(())
    }
    pub fn configure_audio(&mut self, rx_channel: u32) -> Result<(), &'static str> {
        if self.kind != 7 || rx_channel > 1 {
            return Err("invalid SAI wiring");
        }
        self.sai.rx_channel = rx_channel as usize;
        Ok(())
    }
    pub fn new(kind: u32) -> Self {
        let mut result = Self {
            kind,
            registers: BTreeMap::new(),
            dma: dma::Dma::default(),
            sai: sai::Sai::default(),
            qspi: std::sync::Arc::new(std::sync::Mutex::new(qspi::Qspi::default())),
            gpio: gpio::Gpio::default(),
            codec_reset_high: false,
            memory: std::sync::Mutex::new(memory::SystemMemory::default()),
            clock: clock::ClockTree::default(),
            cpu_progress: cpu_progress::CpuProgress::default(),
            line_stage: codec::LineStage::default(),
            passband_delay: codec::PassbandDelay::default(),
            axi: bus::Axi::default(),
            fmc_commands: std::collections::VecDeque::new(),
        };
        result.reset();
        result
    }
    pub fn reset(&mut self) {
        self.reset_with_capture(false);
    }
    fn reset_with_capture(&mut self, preserve_recording: bool) {
        self.memory.lock().unwrap().reset_registers();
        self.clock = clock::ClockTree::default();
        self.cpu_progress = cpu_progress::CpuProgress::default();
        self.line_stage = codec::LineStage::default();
        self.passband_delay = codec::PassbandDelay::default();
        self.axi = bus::Axi::default();
        self.fmc_commands.clear();
        self.registers.clear();
        self.dma = dma::Dma::default();
        self.qspi.lock().unwrap().reset_controller();
        if self.kind >= 100 {
            self.gpio.reset(self.kind - 100);
        }
        self.sai.reset(preserve_recording);
        if self.kind == 0 {
            self.registers
                .extend([(0, 0xF000_C000), (0x0c, 2), (0x18, 0xc000)]);
        }
        if self.kind == 1 {
            self.registers.extend([
                (0x140, 0x2d0),
                (0x144, 0x2d0),
                (0x148, 0x0fffffff),
                (0x14c, 0x0fffffff),
            ]);
        }
        if self.kind == 4 {
            self.registers.extend(
                [0x97, 0x97, 0x79, 0x79, 0x0a, 8, 0x9f, 0x0a, 0, 0]
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| (index as u64, value)),
            );
        }
    }
    pub fn read(&mut self, offset: u64) -> Result<u32, &'static str> {
        match self.kind {
            0 => match offset {
                0 | 8 | 0xc | 0x10 | 0x14 | 0x20 | 0x24 | 0x28 | 0x30 | 0x34 | 0x38 => {
                    let value = self.registers.get(&offset).copied().unwrap_or(0);
                    Ok(if offset == 8 && value & 1 != 0 {
                        value | (1 << 16)
                    } else {
                        value
                    })
                }
                4 => Ok((self.registers[&0x18] & 0xc000) | (1 << 13)),
                0x18 => Ok(self.registers[&0x18] | (1 << 13)),
                _ => Err("unsupported PWR register"),
            },
            1 => match offset {
                0 | 4 | 8 | 0xc | 0x10 | 0x14 | 0x18 | 0x1c | 0x140 | 0x144 | 0x148 | 0x14c
                | 0x150 | 0x154 | 0x158 => Ok(self.registers.get(&offset).copied().unwrap_or(0)),
                _ => Err("unsupported FMC register"),
            },
            2 => match offset {
                0x20 => {
                    let value = self.registers.get(&offset).copied().unwrap_or(0);
                    Ok(value | if value & 1 != 0 { 1 << 8 } else { 0 })
                }
                0x2c => Ok(self.registers.get(&offset).copied().unwrap_or(0)),
                _ => Err("register delegated to the built-in SYSCFG model"),
            },
            3 => match offset {
                0x100 => {
                    let cr = self.registers.get(&0x108).copied().unwrap_or(0);
                    Ok(self.registers.get(&offset).copied().unwrap_or(0)
                        | if cr & (1 << 28) != 0 { 1 << 12 } else { 0 })
                }
                0x104..=0x150 | 0x160..=0x180 | 0x1b0..=0x1c8 if offset.is_multiple_of(4) => {
                    Ok(self.registers.get(&offset).copied().unwrap_or(0))
                }
                _ => Err("register delegated to primary ADC model"),
            },
            4 if offset < 16 => Ok(self.registers.get(&offset).copied().unwrap_or(0)),
            // Host-only codec boundary query. Unsupported active digital formats
            // fail explicitly instead of silently feeding valid-looking PCM.
            4 if offset == 0x100 => {
                let reg = |index| self.registers.get(&index).copied().unwrap_or(0);
                if reg(9) & 1 == 0 {
                    return Ok(0);
                }
                if reg(7) & !(1 << 5) != 9 {
                    return Err("unsupported active WM8731 digital format");
                }
                codec::Sampling::decode(reg(8))?;
                if reg(5) & 6 != 0 {
                    return Err("WM8731 de-emphasis filter is not implemented");
                }
                if reg(4) & 0x2c != 0 {
                    return Err(
                        "WM8731 microphone, bypass and sidetone routing is not implemented",
                    );
                }
                if reg(6) & 0x80 != 0 {
                    return Ok(0);
                }
                let adc = u32::from(reg(6) & 5 == 0);
                let dac = u32::from(reg(6) & 0x18 == 0 && reg(5) & 8 == 0 && reg(4) & 0x10 != 0);
                Ok(adc | (dac << 1) | ((reg(0) & 0x80) >> 5) | ((reg(1) & 0x80) >> 4))
            }
            6 => self.dma.read(offset),
            7 => self.sai.read(offset),
            8 => self.qspi.lock().unwrap().read(offset, 4),
            9 if offset == 0x100 => Ok(if self.codec_reset_high { 3 } else { 0 }),
            10 if offset == 0x100 => Ok(3), // Rev7 hardware mode, fixed LJ24, no I2C.
            12 => self.memory.lock().unwrap().read_scb(offset),
            13 => Ok(self.clock.read(offset)),
            15 => self.axi.read(offset),
            100..=110 => self.gpio.read(offset),
            _ => Err("unsupported peripheral"),
        }
    }
    pub fn i2c_write(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        match self.kind {
            4 => {
                if bytes.len() != 2 {
                    return Err("WM8731 expects a two-byte register write");
                }
                let register = bytes[0] >> 1;
                if register > 9 && register != 15 {
                    return Err("unsupported WM8731 register");
                }
                if register == 15 {
                    self.reset();
                } else {
                    let value = ((bytes[0] as u32 & 1) << 8) | bytes[1] as u32;
                    self.registers.insert(register as u64, value);
                    if register <= 3 && value & 0x100 != 0 {
                        let opposite = u64::from(register ^ 1);
                        let previous = self.registers.get(&opposite).copied().unwrap_or(0);
                        self.registers
                            .insert(opposite, (previous & 0x100) | (value & 0xff));
                    }
                }
            }
            5 => {
                if bytes.is_empty() || ![0x00, 0x40, 0x80].contains(&bytes[0]) {
                    return Err("invalid SSD1306 control byte");
                }
                *self.registers.entry(0).or_default() += 1;
            }
            _ => return Err("not an I2C slave"),
        }
        Ok(())
    }
    pub fn write(&mut self, offset: u64, value: u32) -> Result<(), &'static str> {
        if self.kind == 15 {
            return self.axi.write(offset, value);
        }
        if self.kind == 12 {
            return self.memory.lock().unwrap().write_scb(offset, value);
        }
        if self.kind == 6 {
            return self.dma.write(offset, value);
        }
        if self.kind == 7 {
            return self.sai.write(offset, value);
        }
        if self.kind == 8 {
            return self.qspi.lock().unwrap().write(offset, value, 4);
        }
        if self.kind == 13 {
            self.clock.write(offset, value);
            return Ok(());
        }
        if self.kind >= 100 {
            return self.gpio.write(offset, value);
        }
        self.read(offset)?;
        if self.kind == 0 && offset == 4 {
            return Err("CSR1 is read-only");
        }
        if self.kind == 1 && offset == 0x158 {
            return Err("SDSR is read-only");
        }
        if self.kind == 1 && offset == 0x150 && value & 7 > 6 {
            return Err("invalid SDRAM command");
        }
        if self.kind == 1 && offset == 0x150 {
            self.fmc_commands.push_back(value);
        }
        if self.kind == 1 && offset == 0x154 {
            if value & 1 != 0 {
                self.registers.insert(0x10000, 1);
            }
            self.registers.insert(offset, value & !1);
            return Ok(());
        }
        if self.kind == 3 && offset == 0x108 {
            if value & (1 << 31) != 0 && (value & (1 << 28) == 0 || value & ((1 << 29) | 1) != 0) {
                return Err("invalid ADC calibration state");
            }
            self.registers.insert(offset, value & !(1 << 31));
            return Ok(());
        }
        self.registers.insert(
            offset,
            if self.kind == 0 && offset == 0x18 {
                value & !0x2000
            } else {
                value
            },
        );
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn daisy_new(kind: u32) -> *mut Peripheral {
    Box::into_raw(Box::new(Peripheral::new(kind)))
}

/// # Safety
/// `state` is a live, serialized architecture state; width is 1, 2, or 4.
/// Upper word: 0=data, 1=unsupported, 2=documented DMA read bus error.
#[no_mangle]
pub unsafe extern "C" fn daisy_memory_read(
    state: *mut Peripheral,
    address: u32,
    width: u32,
    cpu: u32,
) -> u64 {
    let result = if cpu == 2 {
        (*state)
            .memory
            .lock()
            .unwrap()
            .decode_instruction(address, width as usize)
    } else if cpu != 0 {
        (*state)
            .memory
            .lock()
            .unwrap()
            .read_cpu(address, width as usize)
    } else {
        (*state)
            .memory
            .lock()
            .unwrap()
            .read_dma(address, width as usize)
    };
    match result {
        Ok(value) => u64::from(value),
        Err(error)
            if cpu == 0 && (qspi::mapped_bus_error(error) || bus::sdram_bus_error(error)) =>
        {
            2_u64 << 32
        }
        Err(_) => 1_u64 << 32,
    }
}

/// # Safety
/// `state` is a live, exclusively accessed architecture state; width is 1, 2, or 4.
#[no_mangle]
pub unsafe extern "C" fn daisy_memory_write(
    state: *mut Peripheral,
    address: u32,
    width: u32,
    value: u32,
    cpu: u32,
) -> u32 {
    let result = if cpu == 2 {
        (*state)
            .memory
            .lock()
            .unwrap()
            .load_flash(address, width as usize, value)
    } else if cpu != 0 {
        (*state)
            .memory
            .lock()
            .unwrap()
            .write_cpu(address, width as usize, value)
    } else {
        (*state)
            .memory
            .lock()
            .unwrap()
            .write_dma(address, width as usize, value)
    };
    match result {
        Ok(()) => 0,
        Err(error) if cpu == 0 && bus::sdram_bus_error(error) => 2,
        Err(_) => 1,
    }
}

/// # Safety
/// Live, serialized architecture state. Called before a translated block runs.
/// Status: 0=ready, 1=retranslate, 2=wait, 3=unsupported, 4=instruction BusFault.
/// A count of zero is a pure translation-policy query: 5=one instruction, 0=default.
#[no_mangle]
pub unsafe extern "C" fn daisy_runtime_fetch(
    state: *mut Peripheral,
    address: u32,
    count: u32,
) -> u32 {
    if count == 0 {
        return if (*state)
            .memory
            .lock()
            .unwrap()
            .single_instruction_translation(address)
        {
            5
        } else {
            0
        };
    }
    let mut memory = (*state).memory.lock().unwrap();
    memory.begin_cpu_block();
    match memory.runtime_fetch(address, count) {
        Ok(false) => {
            if memory.has_cpu_debt() {
                2
            } else {
                0
            }
        }
        Ok(true) => 1,
        Err(error)
            if qspi::mapped_bus_error(error)
                && memory.instruction_bus_fault_at_start(address, count) =>
        {
            4
        }
        Err(_) => 3,
    }
}

/// # Safety
/// Live architecture state. Called only for potentially unaligned CPU accesses.
#[no_mangle]
pub unsafe extern "C" fn daisy_alignment(
    state: *mut Peripheral,
    address: u32,
    alignment: u32,
    forced: u32,
) -> u32 {
    (*state)
        .memory
        .lock()
        .unwrap()
        .alignment_fault(address, alignment, forced != 0)
        .map(u32::from)
        .unwrap_or(u32::MAX)
}

/// # Safety
/// Live architecture state and a valid mutable u64 value. Context is guest PC+SP.
/// Returns service ns, MAX for unsupported behavior, MAX-1 for a modeled bus error.
#[no_mangle]
pub unsafe extern "C" fn daisy_cpu_access(
    state: *mut Peripheral,
    context: u64,
    address: u32,
    width: u32,
    value: *mut u64,
    write: u32,
) -> u64 {
    match (*state).memory.lock().unwrap().cpu_access(
        context,
        address,
        width as usize,
        write != 0,
        &mut *value,
    ) {
        Ok(wait) => wait,
        Err(error) if qspi::mapped_bus_error(error) || bus::sdram_bus_error(error) => u64::MAX - 1,
        Err(error) => {
            eprintln!("CPU bus error: context={context:#x} address={address:#x} width={width} write={write}: {error}");
            u64::MAX
        }
    }
}

/// # Safety
/// Live architecture/RCC/FMC states; callers serialize clock/configuration writes.
#[no_mangle]
pub unsafe extern "C" fn daisy_bus_sync(
    state: *mut Peripheral,
    clock: *mut Peripheral,
    fmc: *mut Peripheral,
    now: u64,
) -> u32 {
    u32::from(
        synchronize_memory(
            &mut (*state).memory.lock().unwrap(),
            &*clock,
            &mut *fmc,
            now,
        )
        .is_err(),
    )
}
/// Synchronize shared memory, SDRAM commands and refresh status with RCC/FMC.
/// Both native and Rust hosts use this single implementation.
pub fn synchronize_memory(
    memory: &mut memory::SystemMemory,
    clock: &Peripheral,
    fmc: &mut Peripheral,
    now: u64,
) -> Result<(), &'static str> {
    let hclk = clock.clock.domain_rate(1)?.ok_or("HCLK stopped")?;
    let pclk = clock.clock.domain_rate(3)?.ok_or("PCLK2 stopped")?;
    let reg = |offset| fmc.registers.get(&offset).copied().unwrap_or(0);
    let enabled =
        clock.clock.read(0xd4) & (1 << 12) != 0 && clock.clock.read(0x7c) & (1 << 12) == 0;
    let timing = if enabled {
        clock
            .clock
            .fmc_rate()?
            .and_then(|r| bus::SdramTiming::decode(reg(0x140), reg(0x148), reg(0x154), r).ok())
    } else {
        None
    };
    let now = now.max(memory.now());
    memory.advance_time(now)?;
    memory.configure_bus(hclk, pclk, timing);
    if fmc.registers.remove(&0x10000).is_some() {
        memory.fabric.clear_sdram_refresh_error();
    }
    while let Some(command) = fmc.fmc_commands.pop_front() {
        memory.fabric.sdram_command(command)?;
    }
    memory.advance_time(now)?;
    fmc.registers.insert(0x158, memory.fabric.sdram_status());
    Ok(())
}

/// # Safety
/// Live serialized FMC state, synchronized with the shared bus beforehand.
#[no_mangle]
pub unsafe extern "C" fn daisy_fmc_irq(state: *mut Peripheral) -> u32 {
    u32::from(
        (*state).registers.get(&0x158).copied().unwrap_or(0) & 1 != 0
            && (*state).registers.get(&0x154).copied().unwrap_or(0) & (1 << 14) != 0,
    )
}

/// # Safety
/// Live architecture state.
#[no_mangle]
pub unsafe extern "C" fn daisy_cpu_debt(state: *mut Peripheral) -> u64 {
    (*state).memory.lock().unwrap().take_cpu_debt()
}
/// # Safety
/// Live architecture state. Called on the CPU thread when an eligible IRQ
/// abandons the unconsumed part of a normal-memory wait budget.
#[no_mangle]
pub unsafe extern "C" fn daisy_abandon_wait(state: *mut Peripheral, ns: u64) {
    (*state).memory.lock().unwrap().abandon_wait(ns);
}
/// # Safety
/// Live architecture state; metadata for the current CPU wait budget.
#[no_mangle]
pub unsafe extern "C" fn daisy_wait_interruptible(state: *mut Peripheral) -> u32 {
    u32::from((*state).memory.lock().unwrap().wait_interruptible)
}

/// # Safety
/// Live architecture state, invoked by the native DSB/DMB instruction adapter.
#[no_mangle]
pub unsafe extern "C" fn daisy_barrier(state: *mut Peripheral, kind: u32) -> u32 {
    match (*state).memory.lock().unwrap().barrier(kind) {
        Ok(0) => 0,
        Ok(_) => 1,
        Err(_) => 2,
    }
}

/// # Safety
/// Live architecture state. Native continuation adapter calls outside TB bodies.
#[no_mangle]
pub unsafe extern "C" fn daisy_exception(state: *mut Peripheral, address: u32, phase: u32) -> u32 {
    let mut memory = (*state).memory.lock().unwrap();
    let result = match phase {
        0 => memory.exception_entry(address),
        1 => memory.finish_exception_entry().map(|_| 0),
        2 => {
            memory.finish_exception_return();
            Ok(0)
        }
        0x120 | 0x168 => memory.exception_return(address, phase - 0x100),
        _ => Err("unsupported native exception phase"),
    };
    match result {
        Ok(0) => 0,
        Ok(_) => 1,
        Err(error) => {
            eprintln!("Exception bus error: address={address:#x} phase={phase:#x}: {error}");
            2
        }
    }
}

/// # Safety
/// Live architecture state; time comes from the emulator's virtual clock.
#[no_mangle]
pub unsafe extern "C" fn daisy_time_sync(state: *mut Peripheral, now: u64) -> u32 {
    let mut memory = (*state).memory.lock().unwrap();
    let now = now.max(memory.now());
    u32::from(memory.advance_time(now).is_err())
}
/// # Safety
/// Live architecture state.
#[no_mangle]
pub unsafe extern "C" fn daisy_flash_irq(state: *mut Peripheral) -> u32 {
    u32::from((*state).memory.lock().unwrap().flash_irq())
}
/// # Safety
/// Live architecture state. Space0=flash, space1=AXI; widths1/2/4 for flash keys.
#[no_mangle]
pub unsafe extern "C" fn daisy_shared_reg(
    state: *mut Peripheral,
    space: u32,
    offset: u64,
    value: u32,
    write: u32,
    width: u32,
) -> u64 {
    let mut memory = (*state).memory.lock().unwrap();
    let result = if space == 0 {
        if write != 0 {
            memory
                .flash_write_reg(offset, value, width as usize)
                .map(|_| 0)
        } else {
            memory.flash_read_reg(offset)
        }
    } else if space == 1 {
        if write != 0 {
            memory.fabric.axi.write(offset, value).map(|_| 0)
        } else {
            memory.fabric.axi.read(offset)
        }
    } else if space == 2 && offset == 0 && write != 0 {
        memory.set_flash_maximum(value != 0).map(|_| 0)
    } else if space == 3 && offset == 0 && write != 0 {
        memory.bypass_exception_waits = value != 0;
        Ok(0)
    } else {
        Err("invalid shared register space")
    };
    match result {
        Ok(v) => u64::from(v),
        Err(error) if space == 0 && flash::bus_error(error) => 2 << 32,
        Err(_) => 1 << 32,
    }
}
/// # Safety
/// Live architecture state. token0 issues a new DMA port operation, other tokens
/// poll completion. 0=ready, 1=admission blocked, other values are live tokens.
/// MAX=unsupported; MAX-1=bus error, which the DMA stream must complete as TEIF.
#[no_mangle]
pub unsafe extern "C" fn daisy_dma_port(
    state: *mut Peripheral,
    address: u32,
    write: u32,
    token: u64,
) -> u64 {
    let mut memory = (*state).memory.lock().unwrap();
    if token != 0 {
        return if memory.dma_ready(token - 2) {
            0
        } else {
            token
        };
    }
    match memory.dma_submit(address, write != 0) {
        Ok(Some(id)) => id + 2,
        Ok(None) => 1,
        Err(error) if qspi::mapped_bus_error(error) || bus::sdram_bus_error(error) => u64::MAX - 1,
        Err(_) => u64::MAX,
    }
}
/// # Safety
/// Live architecture state; deterministic synthetic competing-master traffic.
#[no_mangle]
pub unsafe extern "C" fn daisy_bus_load(
    state: *mut Peripheral,
    target: u32,
    ns: u64,
    write: u32,
) -> u32 {
    u32::from(
        (*state)
            .memory
            .lock()
            .unwrap()
            .inject_bus_load_kind(target as usize, ns, write != 0)
            .is_err(),
    )
}

#[no_mangle]
pub extern "C" fn daisy_memory_is_ram(address: u32, width: u32) -> u32 {
    u32::from(
        memory::SystemMemory::is_ram(address, width as usize)
            || memory::SystemMemory::is_qspi(address, width as usize),
    )
}

/// # Safety
/// `state` is a live, serialized clock state.
#[no_mangle]
pub unsafe extern "C" fn daisy_clock_advance(
    state: *mut Peripheral,
    cr1: u32,
    frcr: u32,
    ns: u64,
) -> u64 {
    (*state)
        .clock
        .advance(cr1, frcr, ns)
        .map_or(1_u64 << 32, u64::from)
}

/// # Safety
/// `state` is a live, serialized clock state.
#[no_mangle]
pub unsafe extern "C" fn daisy_clock_rate(state: *mut Peripheral, cr1: u32, frcr: u32) -> f64 {
    (*state)
        .clock
        .sample_rate(cr1, frcr)
        .map_or(f64::NAN, clock::Rate::hz)
}

/// # Safety
/// `state` is a live, serialized clock state.
#[no_mangle]
pub unsafe extern "C" fn daisy_clock_domain(state: *mut Peripheral, domain: u32) -> u64 {
    (*state).clock.domain_rate(domain).map_or(u64::MAX, |rate| {
        rate.map_or(0, |rate| rate.numerator / rate.denominator)
    })
}

/// # Safety
/// `state` is a live, serialized clock state; use write=2 to synchronize before RCC writes.
#[no_mangle]
pub unsafe extern "C" fn daisy_clock_dwt(
    state: *mut Peripheral,
    offset: u64,
    value: u32,
    write: u32,
    now: u64,
) -> u64 {
    if write == 3 {
        return u64::from((*state).clock.trace_enable(value != 0, now).is_err()) << 32;
    }
    if write == 2 {
        return u64::from((*state).clock.sync_cycles(now).is_err()) << 32;
    }
    (*state)
        .clock
        .dwt(offset, if write == 0 { None } else { Some(value) }, now)
        .map_or(1_u64 << 32, u64::from)
}

/// # Safety
/// `state` is live/serialized; `path` is a valid NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn daisy_clock_save(
    state: *mut Peripheral,
    cr1: u32,
    frcr: u32,
    path: *const std::ffi::c_char,
) -> u32 {
    let Ok(path) = std::ffi::CStr::from_ptr(path).to_str() else {
        return 1;
    };
    let Ok(stats) = (*state).clock.stats(cr1, frcr) else {
        return 1;
    };
    u32::from(std::fs::write(path, stats).is_err())
}

/// # Safety
/// `state` must be live/serialized and `path` a valid NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn daisy_memory_save(
    state: *mut Peripheral,
    path: *const std::ffi::c_char,
) -> u32 {
    let Ok(path) = std::ffi::CStr::from_ptr(path).to_str() else {
        return 1;
    };
    u32::from(std::fs::write(path, (*state).memory.lock().unwrap().stats()).is_err())
}

/// # Safety
/// Distinct live/serialized FMC and clock states; path is NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn daisy_fmc_save(
    fmc: *mut Peripheral,
    clock: *mut Peripheral,
    path: *const std::ffi::c_char,
) -> u32 {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let path = std::ffi::CStr::from_ptr(path).to_str()?;
        let reg = |offset| (*fmc).registers.get(&offset).copied().unwrap_or(0);
        let raw = |status| {
            format!("fmc_state={status}\nrcc_ahb3enr={:#010x}\nsdcr1={:#010x}\nsdtr1={:#010x}\nsdrtr={:#010x}\n",(*clock).clock.read(0xd4),reg(0x140),reg(0x148),reg(0x154))
        };
        if (*clock).clock.read(0xd4) & (1 << 12) == 0 {
            std::fs::write(path, raw("CLOCK_DISABLED"))?;
            return Ok(());
        }
        let Some(rate) = (*clock).clock.fmc_rate()? else {
            std::fs::write(path, raw("CLOCK_STOPPED"))?;
            return Ok(());
        };
        if reg(0x140) == 0x2d0 && reg(0x148) == 0x0fff_ffff && reg(0x154) == 0 {
            std::fs::write(path, raw("UNCONFIGURED"))?;
            return Ok(());
        }
        let timing = bus::SdramTiming::decode(reg(0x140), reg(0x148), reg(0x154), rate)?;
        std::fs::write(path, timing.report())?;
        Ok(())
    })();
    u32::from(result.is_err())
}

/// # Safety
/// `state` must be the live pointer returned by daisy_new, with serialized access.
#[no_mangle]
pub unsafe extern "C" fn daisy_read(state: *mut Peripheral, offset: u64) -> u64 {
    match (*state).read(offset) {
        Ok(value) => value as u64,
        Err(_) => 1_u64 << 32,
    }
}

/// # Safety
/// `state` must be the live pointer returned by daisy_new, with serialized access.
#[no_mangle]
pub unsafe extern "C" fn daisy_write(state: *mut Peripheral, offset: u64, value: u32) -> u32 {
    u32::from((*state).write(offset, value).is_err())
}

/// # Safety
/// `state` must be the live pointer returned by daisy_new, with serialized access.
#[no_mangle]
pub unsafe extern "C" fn daisy_reset(state: *mut Peripheral) {
    // Renode resets the hardware model while the host WAV capture stays open.
    let peripheral = &mut *state;
    peripheral.reset_with_capture(peripheral.kind == 7);
}

/// # Safety
/// `state` must be live; `bytes` must point to `length` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn daisy_i2c_write(
    state: *mut Peripheral,
    bytes: *const u8,
    length: usize,
) -> u32 {
    if length == 0 {
        return 1;
    }
    u32::from(
        (*state)
            .i2c_write(std::slice::from_raw_parts(bytes, length))
            .is_err(),
    )
}

#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_dma_request(state: *mut Peripheral, channel: u32) -> u64 {
    match (*state).dma.request(channel as usize) {
        Ok(Some((src, dst))) => ((src as u64) << 32) | dst as u64,
        Ok(None) => 0,
        Err(_) => u64::MAX,
    }
}

/// # Safety
/// Live serialized DMA state; peek does not advance NDTR or IRQ state.
#[no_mangle]
pub unsafe extern "C" fn daisy_dma_peek(state: *mut Peripheral, stream: u32) -> u64 {
    match (*state).dma.clone().request(stream as usize) {
        Ok(Some((source, destination))) => (u64::from(source) << 32) | u64::from(destination),
        Ok(None) => 0,
        Err(_) => u64::MAX,
    }
}
#[no_mangle]
/// # Safety
/// `state` must be live with serialized access.
pub unsafe extern "C" fn daisy_dma_irqs(state: *mut Peripheral) -> u32 {
    (*state).dma.irqs()
}

/// # Safety
/// Live serialized DMA state; called after a modeled external bus-error response.
#[no_mangle]
pub unsafe extern "C" fn daisy_dma_bus_error(state: *mut Peripheral, stream: u32) -> u32 {
    u32::from((*state).dma.bus_error(stream as usize).is_err())
}

/// # Safety
/// Live and exclusively accessed DMA state, with simultaneous pending requests.
#[no_mangle]
pub unsafe extern "C" fn daisy_dma_grant(state: *mut Peripheral, requests: u32) -> u32 {
    (*state)
        .dma
        .arbitrate(requests)
        .map_or(u32::MAX, |i| i as u32)
}

/// # Safety
/// Live, serialized DMA state and NUL-terminated UTF-8 path.
#[no_mangle]
pub unsafe extern "C" fn daisy_dma_save(
    state: *mut Peripheral,
    path: *const std::ffi::c_char,
) -> u32 {
    let Ok(path) = std::ffi::CStr::from_ptr(path).to_str() else {
        return 1;
    };
    u32::from(std::fs::write(path, (*state).dma.stats()).is_err())
}
#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_sai_tick(state: *mut Peripheral, codec_active: u32) {
    (*state).sai.tick_mask(codec_active);
}

/// # Safety
/// Distinct live serialized codec and RCC states, before processing SAI frames.
#[no_mangle]
pub unsafe extern "C" fn daisy_codec_clocks(
    codec: *mut Peripheral,
    clock: *mut Peripheral,
    cr1: u32,
    frcr: u32,
) -> u32 {
    if ((*codec).kind == 4 && (*codec).registers.get(&9).copied().unwrap_or(0) & 1 == 0)
        || ((*codec).kind == 9 && !(*codec).codec_reset_high)
    {
        return 0;
    }
    let result = (|| {
        let mclk = (*clock).clock.sai_mclk(cr1, frcr)?;
        let frame = (*clock).clock.sample_rate(cr1, frcr)?;
        (*codec).validate_codec_clocks(mclk, frame)
    })();
    u32::from(result.is_err())
}

/// # Safety
/// Both pointers are distinct live states with serialized access.
#[no_mangle]
pub unsafe extern "C" fn daisy_sai_tick_codec(
    state: *mut Peripheral,
    codec: *mut Peripheral,
    ideal_line: u32,
) -> u32 {
    let sample = (*state).sai.input_sample();
    u32::from(
        (*state)
            .audio_frame_with_codec(&mut *codec, sample, ideal_line != 0)
            .is_err(),
    )
}

/// # Safety
/// `state` must be a live, exclusive SAI state. Configure before CPU execution.
#[no_mangle]
pub unsafe extern "C" fn daisy_sai_configure(state: *mut Peripheral, rx_channel: u32) -> u32 {
    u32::from((*state).configure_audio(rx_channel).is_err())
}

/// # Safety
/// `state` must be a live, serialized SAI state. Rate comes from the modeled clock tree.
#[no_mangle]
pub unsafe extern "C" fn daisy_sai_rate(state: *mut Peripheral, rate: f64) -> u32 {
    if !rate.is_finite() || !(1.0..=384000.0).contains(&rate) {
        return 1;
    }
    if (*state).sai.frames != 0
        && (*state).sai.sample_rate != 0.0
        && (*state).sai.sample_rate != rate
    {
        return 1;
    } // A WAV cannot label a variable-rate capture.
    (*state).sai.sample_rate = rate;
    0
}

/// The rate actually used by the captured frames, or zero before audio starts.
/// # Safety
/// `state` must be a live, serialized SAI state.
#[no_mangle]
pub unsafe extern "C" fn daisy_sai_capture_rate(state: *mut Peripheral) -> f64 {
    (*state).sai.sample_rate
}

/// Convert reference CPU progress to nanoseconds while retaining fractional phase.
/// # Safety
/// `state` must be live and exclusively serialized; `nanoseconds` must be writable.
#[no_mangle]
pub unsafe extern "C" fn daisy_cpu_progress(
    state: *mut Peripheral,
    cycles: u64,
    mips: u32,
    nanoseconds: *mut u64,
) -> u32 {
    match (*state).cpu_progress.advance(cycles, mips) {
        Ok(ns) => {
            *nanoseconds = ns;
            0
        }
        Err(_) => 1,
    }
}

/// # Safety
/// `state` must be a live, exclusively accessed codec state.
#[no_mangle]
pub unsafe extern "C" fn daisy_codec_control(state: *mut Peripheral, high: u32) -> u32 {
    u32::from((*state).codec_control(high != 0).is_err())
}

/// # Safety
/// `state` is a live/serialized codec state and `path` is NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn daisy_codec_save(
    state: *mut Peripheral,
    path: *const std::ffi::c_char,
    ideal_line: u32,
    rate: f64,
) -> u32 {
    if !rate.is_finite() || rate <= 0.0 {
        return 1;
    }
    let Ok(path) = std::ffi::CStr::from_ptr(path).to_str() else {
        return 1;
    };
    let Ok(report) = (*state).codec_report(ideal_line != 0, rate) else {
        return 1;
    };
    u32::from(std::fs::write(path, report).is_err())
}

/// # Safety
/// `state` must be a live, exclusively accessed GPIO port D state.
#[no_mangle]
pub unsafe extern "C" fn daisy_revision_straps(state: *mut Peripheral, grounded: u32) -> u32 {
    u32::from((*state).configure_revision_straps(grounded).is_err())
}

/// # Safety
/// `state` must be live and exclusively accessed for this call.
#[no_mangle]
pub unsafe extern "C" fn daisy_sai_fault(state: *mut Peripheral, drop_rx: u32) {
    (*state).sai.drop_rx = drop_rx != 0;
}
#[no_mangle]
/// # Safety
/// `state` must be live with serialized access.
pub unsafe extern "C" fn daisy_sai_requests(state: *mut Peripheral) -> u32 {
    (*state).sai.requests()
}

/// # Safety
/// Live serialized SAI state; returns the shared A/B interrupt level.
#[no_mangle]
pub unsafe extern "C" fn daisy_sai_irq(state: *mut Peripheral) -> u32 {
    u32::from((*state).audio_interrupt())
}
#[no_mangle]
/// # Safety
/// `state` must be exclusive; `samples` must reference `frames * 2` valid floats.
pub unsafe extern "C" fn daisy_sai_input(
    state: *mut Peripheral,
    samples: *const f32,
    frames: usize,
) {
    if frames == 0 {
        (*state).sai.input.clear();
        return;
    }
    (*state).sai.input = std::slice::from_raw_parts(samples, frames * 2)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|s| [s[0], s[1]])
        .collect();
}
#[no_mangle]
/// # Safety
/// `state` must be exclusive; `path` must be a valid NUL-terminated UTF-8 string.
pub unsafe extern "C" fn daisy_sai_save(
    state: *mut Peripheral,
    path: *const std::ffi::c_char,
    sample_rate: u32,
) -> u32 {
    let path = match std::ffi::CStr::from_ptr(path).to_str() {
        Ok(path) => path,
        Err(_) => return 1,
    };
    u32::from(sai_capture::save(&(*state).sai, path, sample_rate).is_err())
}

#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_read_width(state: *mut Peripheral, offset: u64, width: u32) -> u64 {
    match (*state).qspi.lock().unwrap().read(offset, width as usize) {
        Ok(value) => value as u64,
        Err(_) => 1_u64 << 32,
    }
}
#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_write_width(
    state: *mut Peripheral,
    offset: u64,
    value: u32,
    width: u32,
) -> u32 {
    u32::from(
        (*state)
            .qspi
            .lock()
            .unwrap()
            .write(offset, value, width as usize)
            .is_err(),
    )
}

/// # Safety
/// Distinct live QSPI/RCC states. The caller serializes RCC register updates.
#[no_mangle]
pub unsafe extern "C" fn daisy_qspi_sync(
    state: *mut Peripheral,
    clock: *mut Peripheral,
    now: u64,
) -> u32 {
    let result = (|| {
        let mut qspi = (*state).qspi.lock().unwrap();
        qspi.advance_time(now)?;
        let rate = if (*clock).clock.read(0xd4) & (1 << 14) != 0 {
            (*clock).clock.qspi_rate()?
        } else {
            None
        };
        qspi.configure_clock(rate)
    })();
    if let Err(error) = result {
        eprintln!("QSPI synchronization error: {error}");
        return 1;
    }
    0
}
/// # Safety
/// Live QSPI state. No event is represented by u64::MAX.
#[no_mangle]
pub unsafe extern "C" fn daisy_qspi_next_event(state: *mut Peripheral) -> u64 {
    (*state)
        .qspi
        .lock()
        .unwrap()
        .next_event()
        .unwrap_or(u64::MAX)
}
/// # Safety
/// Live QSPI state.
#[no_mangle]
pub unsafe extern "C" fn daisy_qspi_irq(state: *mut Peripheral) -> u32 {
    u32::from((*state).qspi_interrupt())
}
/// # Safety
/// Live serialized architecture/controller states; architecture retains an Arc.
#[no_mangle]
pub unsafe extern "C" fn daisy_qspi_attach(architecture: *mut Peripheral, state: *mut Peripheral) {
    (*architecture)
        .memory
        .lock()
        .unwrap()
        .attach_qspi(std::sync::Arc::clone(&(*state).qspi));
}
/// # Safety
/// Live QSPI state; called after a native CPU block or managed DMA read returns.
#[no_mangle]
pub unsafe extern "C" fn daisy_qspi_schedule_changed(state: *mut Peripheral) -> u32 {
    u32::from((*state).qspi.lock().unwrap().take_schedule_changed())
}
/// # Safety
/// Live serialized QSPI state; lets the managed adapter skip idle timer checks.
#[no_mangle]
pub unsafe extern "C" fn daisy_qspi_watches_memory(state: *mut Peripheral) -> u32 {
    u32::from((*state).qspi.lock().unwrap().watches_memory())
}
/// # Safety
/// Live QSPI state. Set before starting nonvolatile operations.
#[no_mangle]
pub unsafe extern "C" fn daisy_qspi_maximum(state: *mut Peripheral, maximum: u32) -> u32 {
    u32::from(
        (*state)
            .qspi
            .lock()
            .unwrap()
            .set_maximum(maximum != 0)
            .is_err(),
    )
}
#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_gpio_drive(state: *mut Peripheral, pin: u32, high: u32) {
    (*state).gpio.drive(pin, high != 0);
}

#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_gpio_alternate(
    state: *mut Peripheral,
    pin: u32,
    af: u32,
    high: u32,
) {
    (*state).gpio.alternate(pin, af, high != 0);
}
