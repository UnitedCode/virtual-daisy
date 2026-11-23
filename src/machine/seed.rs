use super::{serial::Serial, timer::Timer, wire::I2c, Error};
use crate::{board::Board, clock::Rate, memory::SystemMemory, Peripheral};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use zmu_cortex_m::{bus::Bus, core::fault::Fault};

/// External devices attached to a Seed. Default methods leave them disconnected.
pub trait Carrier {
    /// External input mask and levels for each GPIO port A–K, at this board time.
    fn pins(&mut self, _outputs: &[u16; 11], _now: u64) -> [(u16, u16); 11] {
        [(0, 0); 11]
    }
    /// Complete an I²C write to an external device. Buses are numbered 1 and 2.
    fn i2c_write(&mut self, _bus: u8, _address: u8, _bytes: &[u8]) -> Result<(), Error> {
        Err(Error::new("no I2C device attached at this address"))
    }
    /// Supply a single-ended ADC input in microvolts (0–3,300,000).
    fn adc_input(
        &mut self,
        _adc: u8,
        _channel: u8,
        _outputs: &[u16; 11],
        _now: u64,
    ) -> Result<u32, Error> {
        Ok(0)
    }
    /// Next external input change, in absolute virtual nanoseconds.
    fn next_event(&self) -> Option<u64> {
        None
    }
}
#[derive(Clone, Copy, PartialEq)]
struct Access {
    address: u32,
    width: usize,
    write: bool,
    value: u32,
    fault: Option<Fault>,
}
#[derive(Clone, Copy)]
struct Beat {
    source: u32,
    destination: u32,
    ticket: Option<u64>,
    word: Option<u32>,
}
type FetchAccess = (u32, Result<u16, Fault>);
type Replay = (Vec<Access>, Vec<FetchAccess>);
/// Physical Seed board state used by the Rust execution host.
pub struct Seed {
    pub(super) functional: bool,
    profile: Board,
    clock: Box<Peripheral>,
    fmc: Box<Peripheral>,
    codec: Box<Peripheral>,
    peripherals: BTreeMap<u32, (u32, Peripheral)>,
    gpio: Vec<Peripheral>,
    memory: Box<SystemMemory>,
    itcm: Vec<u8>,
    pub(super) dtcm: Vec<u8>,
    mux: [u32; 16],
    serial: Serial,
    timer: Timer,
    i2c: [I2c; 2],
    adc: BTreeMap<u32, u32>,
    pub(super) carrier: Option<Box<dyn Carrier>>,
    carrier_masks: [u16; 11],
    pub(super) now: u64,
    cycles: u64,
    fraction: u128,
    rate: Rate,
    next_audio: Option<u64>,
    sample_rate: Option<Rate>,
    captured: Vec<[f32; 2]>,
    serial_output: Vec<(u64, u8)>,
    pub(super) reset_requested: bool,
    pub(super) input: super::input::Input,
    pub(super) error: Option<String>,
    pub(super) waiting: bool,
    pub(super) wait_ns: u64,
    context: Option<u64>,
    log: Vec<Access>,
    fetch_log: Vec<FetchAccess>,
    cursor: usize,
    replay: BTreeMap<u64, Replay>,
    phase: u8,
    failed_vector: Option<u32>,
    wait_deadline: u64,
    return_wait: bool,
    beats: [Option<Beat>; 8],
    disables: [Option<u32>; 8],
    pending_dma: u32,
}
impl Seed {
    /// Construct a Rev4, Rev5, or Rev7 board with its physical codec and straps.
    pub fn new(profile: Board) -> Result<Self, Error> {
        let mut peripherals = BTreeMap::new();
        for (base, size, kind) in [
            (0x58024800, 0x400, 0),
            (0x58000400, 0x400, 2),
            (0x40020000, 0x400, 6),
            (0x40015800, 0x400, 7),
            (0x52005000, 0x400, 8),
        ] {
            peripherals.insert(base, (size, Peripheral::new(kind)));
        }
        let mut memory = Box::new(SystemMemory::default());
        peripherals
            .get_mut(&0x52005000)
            .unwrap()
            .1
            .attach_memory(&mut memory)?;
        peripherals
            .get_mut(&0x40015800)
            .unwrap()
            .1
            .configure_audio(profile.rx_channel())?;
        let mut gpio: Vec<_> = (0..11).map(|n| Peripheral::new(100 + n)).collect();
        gpio[3].configure_revision_straps(profile.grounded_straps())?;
        let mut seed = Self {
            functional: false,
            profile,
            clock: Box::new(Peripheral::new(13)),
            fmc: Box::new(Peripheral::new(1)),
            codec: Box::new(Peripheral::new(profile.codec_kind())),
            peripherals,
            gpio,
            memory,
            itcm: vec![0; 65536],
            dtcm: vec![0; 131072],
            mux: [0; 16],
            serial: Serial::default(),
            timer: Timer::default(),
            i2c: core::array::from_fn(|_| I2c::default()),
            adc: BTreeMap::new(),
            carrier: None,
            carrier_masks: [0; 11],
            now: 0,
            cycles: 0,
            fraction: 0,
            rate: Rate {
                numerator: 64_000_000,
                denominator: 1,
            },
            next_audio: None,
            sample_rate: None,
            captured: Vec::new(),
            serial_output: Vec::new(),
            reset_requested: false,
            input: super::input::Input::default(),
            error: None,
            waiting: false,
            wait_ns: 0,
            context: None,
            log: Vec::new(),
            fetch_log: Vec::new(),
            cursor: 0,
            replay: BTreeMap::new(),
            phase: 0,
            failed_vector: None,
            wait_deadline: 0,
            return_wait: false,
            beats: [None; 8],
            disables: [None; 8],
            pending_dma: 0,
        };
        seed.sync_memory()?;
        Ok(seed)
    }
    pub(super) fn loadable(address: u32, width: usize) -> bool {
        Self::tcm(address, width).is_some()
            || SystemMemory::is_ram(address, width)
            || SystemMemory::is_flash(address, width)
    }
    fn tcm(address: u32, width: usize) -> Option<(bool, usize)> {
        let end = u64::from(address).checked_add(width as u64)?;
        if end <= 0x10000 {
            Some((false, address as usize))
        } else if address >= 0x20000000 && end <= 0x20020000 {
            Some((true, (address - 0x20000000) as usize))
        } else {
            None
        }
    }
    pub(super) fn load_bytes(&mut self, address: u32, data: &[u8]) -> Result<(), Error> {
        if !Self::loadable(address, data.len()) {
            return Err(Error::new("firmware segment outside populated memory"));
        }
        for (i, &b) in data.iter().enumerate() {
            let a = address + i as u32;
            if let Some((data_tcm, o)) = Self::tcm(a, 1) {
                if data_tcm {
                    self.dtcm[o] = b;
                } else {
                    self.itcm[o] = b;
                }
            } else if SystemMemory::is_flash(a, 1) {
                self.memory.load_flash(a, 1, u32::from(b))?;
            } else {
                self.memory.write_dma(a, 1, u32::from(b))?;
            }
        }
        Ok(())
    }
    pub(super) fn contains(&self, a: u32) -> bool {
        Self::tcm(a, 1).is_some()
            || SystemMemory::is_ram(a, 1)
            || SystemMemory::is_flash(a, 1)
            || SystemMemory::is_qspi(a, 1)
            || (0x40000000..0x60000000).contains(&a)
            || (0xe0001000..0xe0002000).contains(&a)
            || (0xe000ed78..=0xe000ed84).contains(&a)
            || (0xe000ed90..=0xe000edb8).contains(&a)
            || (0xe000ef50..=0xe000ef78).contains(&a)
    }
    fn sync_memory(&mut self) -> Result<(), &'static str> {
        crate::synchronize_memory(&mut self.memory, &self.clock, &mut self.fmc, self.now)
    }
    pub(super) fn reset_board(&mut self) -> Result<(), Error> {
        self.captured.extend(
            self.peripherals
                .get_mut(&0x40015800)
                .unwrap()
                .1
                .take_audio()?,
        );
        self.serial_output.extend(self.serial.reset(self.now));
        self.clock.reset();
        self.fmc.reset();
        for (_, p) in self.peripherals.values_mut() {
            p.reset();
        }
        for p in &mut self.gpio {
            p.reset();
        }
        self.memory.reset_registers();
        self.timer = Timer::default();
        self.i2c = core::array::from_fn(|_| I2c::default());
        self.adc.clear();
        self.mux = [0; 16];
        self.beats = [None; 8];
        self.disables = [None; 8];
        self.pending_dma = 0;
        self.context = None;
        self.log.clear();
        self.fetch_log.clear();
        self.replay.clear();
        self.cursor = 0;
        self.phase = 0;
        self.failed_vector = None;
        self.wait_deadline = self.now;
        self.return_wait = false;
        self.waiting = false;
        self.wait_ns = 0;
        self.reset_requested = false;
        self.rate = Rate {
            numerator: 64_000_000,
            denominator: 1,
        };
        self.fraction = 0;
        self.next_audio = None;
        self.sample_rate = None;
        self.sync_memory()?;
        Ok(())
    }
    fn gate(address: u32) -> Option<(u64, u64, u32)> {
        Some(match address {
            0x58020000..=0x58022bff => (0xe0, 0x88, 1 << ((address - 0x58020000) / 0x400)),
            0x52004000..=0x52004fff => (0xd4, 0x7c, 1 << 12),
            0x52005000..=0x520053ff => (0xd4, 0x7c, 1 << 14),
            0x40020000..=0x400203ff | 0x40020800..=0x4002083f => (0xd8, 0x80, 1),
            0x40015800..=0x400158ff => (0xf0, 0x98, 1 << 22),
            0x40011000..=0x400113ff => (0xf0, 0x98, 1 << 4),
            0x40000000..=0x400003ff => (0xe8, 0x90, 1),
            0x40005400..=0x400057ff => (0xe8, 0x90, 1 << 21),
            0x40005800..=0x40005bff => (0xe8, 0x90, 1 << 22),
            0x40022000..=0x400223ff => (0xd8, 0x80, 1 << 5),
            0x58000400..=0x580007ff => (0xf4, 0x9c, 1 << 1),
            _ => return None,
        })
    }
    fn active(&self, address: u32) -> bool {
        Self::gate(address).is_none_or(|(enable, reset, mask)| {
            self.clock.clocks().read(enable) & mask != 0
                && self.clock.clocks().read(reset) & mask == 0
        })
    }
    fn resets(&mut self, offset: u32, value: u32) {
        for (&base, (_, p)) in &mut self.peripherals {
            if Self::gate(base)
                .is_some_and(|(_, reset, mask)| reset == u64::from(offset) && value & mask != 0)
            {
                p.reset();
            }
        }
        if offset == 0x7c && value & (1 << 12) != 0 {
            self.fmc.reset();
        }
        if offset == 0x88 {
            for (port, gpio) in self.gpio.iter_mut().enumerate() {
                if value & (1 << port) != 0 {
                    gpio.reset();
                }
            }
        }
        if offset == 0x80 && value & 1 != 0 {
            self.beats = [None; 8];
            self.disables = [None; 8];
            self.pending_dma = 0;
            self.mux = [0; 16];
        }
        if offset == 0x80 && value & (1 << 5) != 0 {
            self.adc.clear();
        }
        if offset == 0x90 {
            if value & 1 != 0 {
                self.timer = Timer::default();
            }
            for (bus, i2c) in self.i2c.iter_mut().enumerate() {
                if value & (1 << (21 + bus)) != 0 {
                    *i2c = I2c::default();
                }
            }
        }
        if offset == 0x98 && value & (1 << 4) != 0 {
            self.serial_output.extend(self.serial.reset(self.now));
        }
    }
    pub(super) fn synchronize(&mut self, cycles: u64) -> Result<(), Error> {
        if self.functional && cycles == self.cycles {
            return Ok(());
        }
        let delta = cycles
            .checked_sub(self.cycles)
            .ok_or("CPU clock moved backwards")?;
        self.cycles = cycles;
        self.fraction += u128::from(delta) * u128::from(self.rate.denominator) * 1_000_000_000;
        let ns = self.fraction / u128::from(self.rate.numerator);
        self.fraction %= u128::from(self.rate.numerator);
        self.now = self
            .now
            .checked_add(ns as u64)
            .ok_or("board time overflow")?;
        if !self.functional {
            self.sync_memory()?;
        }
        self.serial.advance(self.now);
        self.timer.advance(
            ns as u64,
            if self.active(0x40000000) {
                self.clock.domain_hz(6)?
            } else {
                0.
            },
        );
        let qrate = self.clock.qspi_clock()?;
        self.peripherals
            .get_mut(&0x52005000)
            .unwrap()
            .1
            .advance_qspi(self.now, qrate)?;
        let (cr, frame) = self.audio_clock_registers()?;
        let count = self.clock.audio_clock(cr, frame, ns as u64)?;
        self.next_audio = self.clock.next_audio_ns(cr, frame)?.map(|ns| self.now + ns);
        if count > 0 {
            let mclk = self.clock.clocks().sai_mclk(cr, frame)?;
            let rate = self.clock.clocks().sample_rate(cr, frame)?;
            self.sample_rate = Some(rate);
            self.codec.validate_codec_clocks(mclk, rate)?;
            for _ in 0..count {
                let sample = self.input.next(rate);
                self.peripherals
                    .get_mut(&0x40015800)
                    .unwrap()
                    .1
                    .audio_frame_with_codec(&mut self.codec, sample, false)?;
                self.pump_dma()?;
            }
        }
        self.pump_dma()?;
        if !self.functional
            || self
                .carrier
                .as_ref()
                .and_then(|c| c.next_event())
                .is_some_and(|t| t <= self.now)
        {
            self.update_carrier()?;
        }
        Ok(())
    }
    fn audio_clock_registers(&mut self) -> Result<(u32, u32), &'static str> {
        let sai = &mut self.peripherals.get_mut(&0x40015800).unwrap().1;
        for base in [0, 32] {
            let cr = sai.read(base + 4)?;
            if cr & (1 << 16) != 0 && cr & 2 == 0 {
                return Ok((cr, sai.read(base + 0xc)?));
            }
        }
        Ok((sai.read(4)? & !(1 << 16), sai.read(0xc)?))
    }
    pub(super) fn next_cycles(&self, maximum_ns: u64) -> Result<u32, Error> {
        let mut ns = maximum_ns.max(1);
        // 100 us is also the bounded fallback for peripherals without an IRQ deadline.
        ns = ns.min(100_000);
        if self.beats.iter().any(Option::is_some) {
            ns = ns.min(1000);
        }
        for event in [
            self.serial.next(),
            self.peripherals[&0x52005000].1.next_qspi_event(),
            self.carrier.as_ref().and_then(|c| c.next_event()),
        ]
        .into_iter()
        .flatten()
        {
            ns = ns.min(event.saturating_sub(self.now).max(1));
        }
        if let Some(t) = self.timer.next_ns(self.clock.domain_hz(6)?) {
            ns = ns.min(t);
        }
        // The SAI deadline is read without mutating FIFO state.
        if let Some(next) = self.next_audio {
            ns = ns.min(next.saturating_sub(self.now).max(1));
        }
        let cycles = (u128::from(ns) * u128::from(self.rate.numerator))
            .div_ceil(u128::from(self.rate.denominator) * 1_000_000_000)
            .max(1);
        Ok(cycles.min(u128::from(u32::MAX)) as u32)
    }
    pub(super) fn interrupts(&mut self) -> [(usize, bool); 14] {
        let dma = self.peripherals[&0x40020000].1.dma_interrupts();
        let other = [
            (4, self.memory.flash_irq()),
            (
                48,
                self.fmc.read(0x158).unwrap_or(0) & 1 != 0
                    && self.fmc.read(0x154).unwrap_or(0) & (1 << 14) != 0,
            ),
            (87, self.peripherals[&0x40015800].1.audio_interrupt()),
            (92, self.peripherals[&0x52005000].1.qspi_interrupt()),
            (37, self.serial.irq(self.now)),
            (28, self.timer.irq()),
        ];
        std::array::from_fn(|i| {
            if i < 8 {
                ([11, 12, 13, 14, 15, 16, 17, 47][i], dma & (1 << i) != 0)
            } else {
                other[i - 8]
            }
        })
    }
    fn outputs(&mut self) -> Result<[u16; 11], &'static str> {
        let mut r = [0; 11];
        for (i, p) in self.gpio.iter_mut().enumerate() {
            r[i] = p.read(0x10)? as u16;
        }
        Ok(r)
    }
    fn update_carrier(&mut self) -> Result<(), &'static str> {
        let outputs = self.outputs()?;
        if let Some(carrier) = &mut self.carrier {
            let pins = carrier.pins(&outputs, self.now);
            for (port, (mask, levels)) in pins.into_iter().enumerate() {
                for pin in 0..16 {
                    if mask & (1 << pin) != 0 {
                        self.gpio[port].drive_pin(pin, levels & (1 << pin) != 0)?;
                    } else if self.carrier_masks[port] & (1 << pin) != 0 {
                        self.gpio[port].release_pin(pin)?;
                    }
                }
                self.carrier_masks[port] = mask;
            }
        }
        Ok(())
    }
    pub(super) fn drive_pin(&mut self, port: u8, pin: u8, high: bool) -> Result<(), &'static str> {
        if port > 10 || pin > 15 {
            return Err("GPIO port/pin outside A0..K15");
        }
        self.gpio[port as usize].drive_pin(u32::from(pin), high)
    }
    pub(super) fn receive_serial(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        self.serial
            .receive(bytes, self.now, self.clock.domain_hz(3)? as u64)
    }
    pub(super) fn take_serial(&mut self) -> Vec<(u64, u8)> {
        let mut output = std::mem::take(&mut self.serial_output);
        output.extend(self.serial.take());
        output
    }
    pub(super) fn take_audio(&mut self) -> Vec<[f32; 2]> {
        let mut output = std::mem::take(&mut self.captured);
        output.extend(
            self.peripherals
                .get_mut(&0x40015800)
                .unwrap()
                .1
                .take_audio()
                .unwrap_or_default(),
        );
        output
    }
    pub(super) fn sample_rate(&self) -> Option<Rate> {
        self.sample_rate
    }
    pub(super) fn nonvolatile_image(&self) -> Vec<u8> {
        self.peripherals[&0x52005000].1.nonvolatile_image()
    }
    pub(super) fn nonvolatile_revision(&self) -> u64 {
        self.peripherals[&0x52005000].1.nonvolatile_revision()
    }
    pub(super) fn restore_nonvolatile_image(&mut self, image: &[u8]) -> Result<(), &'static str> {
        self.peripherals
            .get_mut(&0x52005000)
            .unwrap()
            .1
            .restore_nonvolatile_image(image)
    }
    pub(super) fn contention(
        &mut self,
        target: usize,
        ns: u64,
        write: bool,
    ) -> Result<(), &'static str> {
        self.memory.inject_bus_load_kind(target, ns, write)
    }
    pub(super) fn memory_report(&self) -> String {
        format!(
            "{}{}",
            self.memory.stats(),
            self.clock.clocks().audio_observations().report()
        )
    }
    pub(super) fn host_read(&mut self, a: u32, w: usize) -> Result<u32, &'static str> {
        self.read(a, w, false)
    }
    pub(super) fn host_write(&mut self, a: u32, w: usize, v: u32) -> Result<(), &'static str> {
        self.write(a, w, v, false)
    }
    fn tcm_read(&self, a: u32, w: usize) -> Option<u32> {
        let (data, o) = Self::tcm(a, w)?;
        let bytes = if data { &self.dtcm } else { &self.itcm };
        Some(
            bytes[o..o + w]
                .iter()
                .enumerate()
                .fold(0, |v, (i, b)| v | (u32::from(*b) << (8 * i))),
        )
    }
    fn read(&mut self, a: u32, w: usize, cpu: bool) -> Result<u32, &'static str> {
        if let Some(v) = self.tcm_read(a, w) {
            return Ok(v);
        }
        if SystemMemory::is_ram(a, w) || SystemMemory::is_flash(a, w) || SystemMemory::is_qspi(a, w)
        {
            return if cpu {
                self.memory.read_cpu(a, w)
            } else {
                self.memory.read_dma(a, w)
            };
        }
        if !self.active(a) {
            return Ok(0);
        }
        if (0x58020000..0x58022c00).contains(&a) {
            let p = ((a - 0x58020000) / 0x400) as usize;
            self.update_carrier()?;
            return self.gpio[p].read(u64::from((a - 0x58020000) % 0x400));
        }
        // The HAL reads RDR as u8/u16. Each access consumes exactly one received character.
        if (w == 1 || w == 2) && a == 0x40011024 {
            return self
                .serial
                .read(0x24, self.now)
                .map(|value| value & if w == 1 { 0xff } else { 0xffff });
        }
        if w != 4 && !(0x52005000..0x52005034).contains(&a) {
            return Err("unsupported peripheral access width");
        }
        match a {
            0x58024400..=0x580247ff => self.clock.read(u64::from(a - 0x58024400)),
            0x52004000..=0x52004fff => self.fmc.read(u64::from(a - 0x52004000)),
            0x52002000..=0x520021ff => self.memory.flash_read_reg(u64::from(a - 0x52002000)),
            0x51000000..=0x510fffff => self.memory.fabric.axi.read(u64::from(a - 0x51000000)),
            0x40011000..=0x4001102c => self.serial.read(a - 0x40011000, self.now),
            0x40000000..=0x40000050 => self.timer.read(a - 0x40000000),
            0x40020800..=0x4002083f => Ok(self.mux[((a - 0x40020800) / 4) as usize]),
            0x40005400..=0x40005bff => {
                self.i2c[((a - 0x40005400) / 0x400) as usize].read((a - 0x40005400) % 0x400)
            }
            0x40022000..=0x400223ff => self.adc_read(a - 0x40022000),
            0xe0001000..=0xe0001fff => self.clock.dwt(u64::from(a - 0xe0001000), None, self.now),
            0xe000ed90 => Ok(16 << 8),
            0xe000ed94..=0xe000edb8 => self.memory.read_mpu(u64::from(a - 0xe000e000)),
            0xe000ed78..=0xe000ed84 => self.memory.read_scb(u64::from(a - 0xe000e000)),
            _ => {
                if let Some((&base, (size, p))) = self.peripherals.range_mut(..=a).next_back() {
                    if a - base < *size {
                        if base == 0x52005000 {
                            return p.qspi_read(u64::from(a - base), w);
                        }
                        return p.read(u64::from(a - base));
                    }
                }
                Err("unmapped peripheral read")
            }
        }
    }
    fn write(&mut self, a: u32, w: usize, v: u32, cpu: bool) -> Result<(), &'static str> {
        if self.functional
            && ((0x52002000..0x52002200).contains(&a)
                || (0x40020010..0x400200d0).contains(&a)
                    && (a - 0x40020010).is_multiple_of(0x18)
                    && v & 1 != 0)
        {
            return Err("flash programming and DMA require the timed runtime");
        }
        if let Some((data, o)) = Self::tcm(a, w) {
            let memory = if data { &mut self.dtcm } else { &mut self.itcm };
            memory[o..o + w].copy_from_slice(&v.to_le_bytes()[..w]);
            return Ok(());
        }
        if SystemMemory::is_ram(a, w) || SystemMemory::is_flash(a, w) || SystemMemory::is_qspi(a, w)
        {
            return if cpu {
                self.memory.write_cpu(a, w, v)
            } else {
                self.memory.write_dma(a, w, v)
            };
        }
        if !self.active(a) {
            return Ok(());
        }
        if (0x58020000..0x58022c00).contains(&a) && w == 4 {
            let p = ((a - 0x58020000) / 0x400) as usize;
            self.gpio[p].write(u64::from((a - 0x58020000) % 0x400), v)?;
            if p == 1 && self.profile == Board::Rev4 {
                let high = self.gpio[1].read(0x10)? & (1 << 11) != 0;
                self.codec.codec_control(high)?;
            }
            if self.functional {
                self.update_carrier()?;
            }
            return Ok(());
        }
        // TDR accepts the same byte/halfword accesses used by embedded-hal's serial Tx.
        if (w == 1 || w == 2) && a == 0x40011028 {
            return self.serial.write(
                0x28,
                v & if w == 1 { 0xff } else { 0xffff },
                self.now,
                self.clock.domain_hz(3)? as u64,
            );
        }
        if w != 4 && !(0x52005000..0x52005034).contains(&a) {
            return Err("unsupported peripheral access width");
        }
        match a {
            0x58024400..=0x580247ff => {
                self.resets(a - 0x58024400, v);
                self.clock.dwt(0, None, self.now)?;
                self.clock
                    .write(u64::from(a - 0x58024400), rcc_ready(a - 0x58024400, v))?;
                self.rate = self
                    .clock
                    .clocks()
                    .core_rate()?
                    .ok_or("core clock stopped")?;
                self.fraction = 0;
                self.sync_memory()?;
            }
            0x52004000..=0x52004fff => {
                self.fmc.write(u64::from(a - 0x52004000), v)?;
                self.sync_memory()?;
            }
            0x52002000..=0x520021ff => {
                self.memory
                    .flash_write_reg(u64::from(a - 0x52002000), v, w)?
            }
            0x51000000..=0x510fffff => {
                self.memory.fabric.axi.write(u64::from(a - 0x51000000), v)?
            }
            0x40011000..=0x4001102c => {
                self.serial
                    .write(a - 0x40011000, v, self.now, self.clock.domain_hz(3)? as u64)?
            }
            0x40000000..=0x40000050 => self.timer.write(a - 0x40000000, v)?,
            0x40020800..=0x4002083f => {
                if v & !127 != 0 {
                    return Err("DMAMUX synchronization is unsupported");
                }
                self.mux[((a - 0x40020800) / 4) as usize] = v;
            }
            0x40005400..=0x40005bff => {
                let bus = ((a - 0x40005400) / 0x400) as usize;
                if let Some((address, bytes)) = self.i2c[bus].write((a - 0x40005400) % 0x400, v)? {
                    if bus == 1 && address == 0x1a && self.profile == Board::Rev5 {
                        self.codec.i2c_write(&bytes)?;
                    } else if let Some(carrier) = &mut self.carrier {
                        if let Err(e) = carrier.i2c_write(bus as u8 + 1, address, &bytes) {
                            self.error = Some(e.to_string());
                            return Err("carrier I2C transaction failed");
                        }
                    } else {
                        return Err("no I2C device attached at this address");
                    }
                }
            }
            0x40022000..=0x400223ff => self.adc_write(a - 0x40022000, v)?,
            0xe0001000..=0xe0001fff => {
                self.clock
                    .dwt(u64::from(a - 0xe0001000), Some(v), self.now)?;
            }
            0xe000ed78..=0xe000edb8 | 0xe000ef50..=0xe000ef78 => {
                self.memory.write_scb(u64::from(a - 0xe000e000), v)?
            }
            _ => {
                if let Some((&base, (size, p))) = self.peripherals.range_mut(..=a).next_back() {
                    if a - base < *size {
                        if base == 0x52005000 {
                            p.qspi_write(u64::from(a - base), w, v)?;
                        } else {
                            let offset = u64::from(a - base);
                            let mut value = v;
                            if base == 0x40020000 && offset >= 0x10 {
                                let stream = ((offset - 0x10) / 0x18) as usize;
                                let register = (offset - 0x10) % 0x18;
                                if stream < 8 && self.beats[stream].is_some() {
                                    if matches!(register, 12 | 16) {
                                        let cr = p.read(0x10 + stream as u64 * 0x18)?;
                                        if cr & (1 << 18) != 0
                                            && ((register == 16) == (cr & (1 << 19) != 0))
                                        {
                                            return Err("active DMA double-buffer rewrite during an in-flight beat");
                                        }
                                    }
                                    if register == 0 && value & 1 == 0 {
                                        self.disables[stream] = Some(value);
                                        value |= 1;
                                    }
                                }
                            }
                            p.write(offset, value)?;
                        }
                        self.pump_dma()?;
                        return Ok(());
                    }
                }
                return Err("unmapped peripheral write");
            }
        }
        Ok(())
    }
    fn stall(&mut self, ns: u64) -> Fault {
        self.memory.take_cpu_debt();
        self.waiting = true;
        self.wait_ns = ns.max(1);
        self.wait_deadline = self.now.saturating_add(self.wait_ns);
        if let Some(context) = self.context {
            self.replay
                .insert(context, (self.log.clone(), self.fetch_log.clone()));
        }
        Fault::Preciserr
    }
    fn failure(&mut self, a: u32, w: usize, write: bool, error: &str) -> Fault {
        if crate::qspi::mapped_bus_error(error)
            || crate::bus::sdram_bus_error(error)
            || crate::flash::bus_error(error)
        {
            if self.phase == 2 {
                self.failed_vector = Some(a);
            }
            return Fault::Preciserr;
        }
        if self.error.is_none() {
            self.error = Some(format!(
                "{}{} at 0x{a:08x}: {error}",
                if write { "write" } else { "read" },
                w
            ));
        }
        Fault::Preciserr
    }
    fn cpu_transfer(&mut self, a: u32, w: usize, write: bool, v: u32) -> Result<u32, Fault> {
        let previous = self.log.get(self.cursor).copied();
        if let Some(previous) = previous {
            if (previous.address, previous.width, previous.write) != (a, w, write)
                || (write && previous.value != v)
            {
                return Err(self.failure(
                    a,
                    w,
                    write,
                    "instruction retry changed its access sequence",
                ));
            }
            self.cursor += 1;
            return previous.fault.map_or(Ok(previous.value), Err);
        }
        let mut value = u64::from(v);
        let result = if SystemMemory::is_ram(a, w)
            || SystemMemory::is_flash(a, w)
            || SystemMemory::is_qspi(a, w)
        {
            let context = match self.phase {
                1 => u64::MAX - 1,
                2 => u64::MAX,
                3 => u64::MAX - 2,
                _ => self.context.unwrap_or(0),
            };
            if self.context.is_some() && self.phase == 0 {
                match self.memory.access_wait(a, w, write, false) {
                    Ok(0) => {
                        if write {
                            self.memory.write_cpu(a, w, v).map(|()| v)
                        } else {
                            self.memory.read_cpu(a, w)
                        }
                    }
                    Ok(ns) => return Err(self.stall(ns)),
                    Err(e) => Err(e),
                }
            } else if self.context.is_some() {
                match self.memory.cpu_access(context, a, w, write, &mut value) {
                    Ok(0) => Ok(value as u32),
                    Ok(ns) => return Err(self.stall(ns)),
                    Err(e) => Err(e),
                }
            } else if write {
                self.memory.write_cpu(a, w, v).map(|()| v)
            } else {
                self.memory.read_cpu(a, w)
            }
        } else if write {
            self.write(a, w, v, true).map(|()| v)
        } else {
            self.read(a, w, true)
        };
        match result {
            Ok(value) => {
                if previous.is_none() {
                    self.log.push(Access {
                        address: a,
                        width: w,
                        write,
                        value,
                        fault: None,
                    });
                }
                self.cursor += 1;
                Ok(value)
            }
            Err(error) => {
                let fault = self.failure(a, w, write, error);
                if self.error.is_none() {
                    self.log.push(Access {
                        address: a,
                        width: w,
                        write,
                        value: v,
                        fault: Some(fault),
                    });
                    self.cursor += 1;
                }
                Err(fault)
            }
        }
    }
    fn fetch(&mut self, a: u32) -> Result<u16, Fault> {
        if let Some(&(_, result)) = self.fetch_log.iter().find(|&&(address, _)| address == a) {
            return result;
        }
        let result = self.fetch_inner(a);
        if !self.waiting {
            self.fetch_log.push((a, result));
        }
        result
    }
    fn fetch_inner(&mut self, a: u32) -> Result<u16, Fault> {
        if let Some(value) = self.tcm_read(a, 2) {
            return Ok(value as u16);
        }
        if !(SystemMemory::is_flash(a, 2)
            || SystemMemory::is_ram(a, 2)
            || SystemMemory::is_qspi(a, 2))
        {
            return Err(self.failure(a, 2, false, "instruction fetch outside populated memory"));
        }
        self.memory
            .decode_instruction(a, 2)
            .map_err(|e| self.failure(a, 2, false, e))?;
        match self.memory.runtime_fetch(a, 2) {
            Ok(_) => {
                if self.memory.has_cpu_debt() {
                    let ns = self.memory.take_cpu_debt();
                    return Err(self.stall(ns));
                }
            }
            Err(e) => return Err(self.failure(a, 2, false, e)),
        }
        let value = self
            .memory
            .read_instruction(a, 2)
            .map(|v| v as u16)
            .map_err(|e| self.failure(a, 2, false, e))?;
        // The modeled instruction service already transferred this line. Hold
        // both halves of a Thumb-32 word together instead of requesting it twice.
        // A word crossing the service-line boundary needs its second transaction.
        if value >> 11 >= 0b11101 {
            if let Some(next) = a.checked_add(2).filter(|&next| next & !31 == a & !31) {
                if self.memory.decode_instruction(next, 2).is_ok() {
                    if let Ok(second) = self.memory.read_instruction(next, 2) {
                        self.fetch_log.push((next, Ok(second as u16)));
                    }
                }
            }
        }
        Ok(value)
    }
    pub(super) fn request_dma(&mut self, stream: usize) -> Result<(), &'static str> {
        if stream >= 8 {
            return Err("DMA stream outside 0..7");
        }
        self.pending_dma |= 1 << stream;
        self.pump_dma()
    }
    fn pump_dma(&mut self) -> Result<(), &'static str> {
        if self.functional {
            return Ok(());
        }
        for _ in 0..64 {
            let mut progressed = false;
            for stream in 0..8 {
                if let Some(mut beat) = self.beats[stream] {
                    let write = beat.word.is_some();
                    let address = if write { beat.destination } else { beat.source };
                    if let Some(ticket) = beat.ticket {
                        if !self.memory.dma_ready(ticket) {
                            continue;
                        }
                        beat.ticket = None;
                        if let Some(word) = beat.word {
                            if let Err(error) = self.dma_write(beat.destination, word) {
                                self.dma_error(stream, error)?;
                                progressed = true;
                                continue;
                            }
                            self.peripherals
                                .get_mut(&0x40020000)
                                .unwrap()
                                .1
                                .dma_complete(stream)?;
                            self.beats[stream] = None;
                            if let Some(value) = self.disables[stream].take() {
                                self.peripherals
                                    .get_mut(&0x40020000)
                                    .unwrap()
                                    .1
                                    .write(0x10 + stream as u64 * 0x18, value)?;
                            }
                        } else {
                            match self.dma_read(beat.source) {
                                Ok(word) => beat.word = Some(word),
                                Err(error) => {
                                    self.dma_error(stream, error)?;
                                    progressed = true;
                                    continue;
                                }
                            }
                            self.beats[stream] = Some(beat);
                        }
                        progressed = true;
                        continue;
                    }
                    match self.memory.dma_submit(address, write) {
                        Ok(Some(ticket)) => {
                            beat.ticket = Some(ticket);
                            self.beats[stream] = Some(beat);
                            progressed = true;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            self.dma_error(stream, error)?;
                            progressed = true;
                        }
                    }
                }
            }
            let lines = self.peripherals[&0x40015800].1.audio_requests();
            let mut requests = if self.beats.iter().all(Option::is_none) {
                self.pending_dma
            } else {
                0
            };
            for s in 0..8 {
                if self.beats.iter().all(Option::is_none)
                    && ((self.mux[s] == 87 && lines & 1 != 0)
                        || (self.mux[s] == 88 && lines & 2 != 0))
                {
                    requests |= 1 << s;
                }
            }
            if let Some((stream, source, destination)) = self
                .peripherals
                .get_mut(&0x40020000)
                .unwrap()
                .1
                .dma_transfer(requests)?
            {
                self.pending_dma &= !(1 << stream);
                self.beats[stream] = Some(Beat {
                    source,
                    destination,
                    ticket: None,
                    word: None,
                });
                progressed = true;
            }
            if !progressed {
                return Ok(());
            }
        }
        Err("DMA service failed to settle")
    }
    fn dma_read(&mut self, a: u32) -> Result<u32, &'static str> {
        if matches!(a, 0x40015820 | 0x40015840) {
            self.peripherals
                .get_mut(&0x40015800)
                .unwrap()
                .1
                .read(u64::from(a - 0x40015800))
        } else {
            self.memory.read_dma(a, 4)
        }
    }
    fn dma_error(&mut self, stream: usize, error: &'static str) -> Result<(), &'static str> {
        if !crate::qspi::mapped_bus_error(error) && !crate::bus::sdram_bus_error(error) {
            return Err(error);
        }
        self.peripherals
            .get_mut(&0x40020000)
            .unwrap()
            .1
            .dma_bus_error(stream)?;
        self.beats[stream] = None;
        self.disables[stream] = None;
        Ok(())
    }
    fn dma_write(&mut self, a: u32, v: u32) -> Result<(), &'static str> {
        if matches!(a, 0x40015820 | 0x40015840) {
            self.peripherals
                .get_mut(&0x40015800)
                .unwrap()
                .1
                .write(u64::from(a - 0x40015800), v)
        } else {
            self.memory.write_dma(a, 4, v)
        }
    }
    fn adc_read(&self, o: u32) -> Result<u32, &'static str> {
        if !o.is_multiple_of(4) || o > 0x308 || (0x200..0x300).contains(&o) {
            return Err("unsupported ADC register");
        }
        if matches!(o, 8 | 0x108) {
            Ok(self.adc.get(&o).copied().unwrap_or(1 << 29))
        } else {
            Ok(self.adc.get(&o).copied().unwrap_or(0))
        }
    }
    fn adc_write(&mut self, o: u32, mut v: u32) -> Result<(), &'static str> {
        self.adc_read(o)?;
        let base = o & !0xff;
        if o.is_multiple_of(0x100) {
            let status = self.adc.entry(o).or_default();
            *status &= !v;
            return Ok(());
        }
        if o % 0x100 == 8 {
            if v & (1 << 31) != 0 {
                if v & (1 << 28) == 0 || v & ((1 << 29) | 1) != 0 {
                    return Err("ADC calibration before power up");
                }
                v &= !(1 << 31);
            }
            if v & (1 << 28) != 0 {
                *self.adc.entry(base).or_default() |= 1 << 12;
            }
            if v & 1 != 0 {
                *self.adc.entry(base).or_default() |= 1;
            }
            if v & 4 != 0 {
                let cfg = *self.adc.get(&(base + 0xc)).unwrap_or(&0);
                let sequence = *self.adc.get(&(base + 0x30)).unwrap_or(&0);
                if v & 1 == 0 || sequence & 15 != 0 || cfg & ((1 << 13) | (1 << 10)) != 0 {
                    return Err(
                        "ADC supports enabled single-ended single-channel software conversions",
                    );
                }
                let channel = ((sequence >> 6) & 31) as u8;
                if channel > 19 {
                    return Err("ADC channel outside 0..19");
                }
                let outputs = self.outputs()?;
                let microvolts = if let Some(carrier) = &mut self.carrier {
                    carrier
                        .adc_input((base / 0x100 + 1) as u8, channel, &outputs, self.now)
                        .map_err(|_| "carrier ADC input failed")?
                } else {
                    0
                };
                if microvolts > 3_300_000 {
                    return Err("ADC input exceeds 3.3V");
                }
                let bits = match (cfg >> 2) & 7 {
                    0 => 16,
                    1 => 14,
                    2 => 12,
                    3 => 10,
                    4 => 8,
                    _ => return Err("unsupported ADC resolution"),
                };
                self.adc.insert(
                    base + 0x40,
                    ((u64::from(microvolts) * ((1u64 << bits) - 1)) / 3_300_000) as u32,
                );
                *self.adc.entry(base).or_default() |= 12;
                v &= !4;
            }
        }
        self.adc.insert(o, v);
        Ok(())
    }
}

pub(super) struct Device(pub Rc<RefCell<Seed>>);
impl Bus for Device {
    fn systick_reference_divider(&self) -> u32 {
        8
    }
    fn validate_access(
        &self,
        address: u32,
        width: usize,
        kind: u8,
        privileged: bool,
        bypass: bool,
    ) -> Result<(), Fault> {
        let mut s = self.0.borrow_mut();
        if s.functional {
            return Ok(());
        }
        if s.phase == 2 {
            return Ok(());
        } // Vector reads bypass MPU permissions.
        for offset in 0..width {
            let (allowed, device) = s
                .memory
                .cpu_permission(
                    address.wrapping_add(offset as u32),
                    kind == 1,
                    kind == 2,
                    privileged,
                    bypass,
                )
                .map_err(|e| s.failure(address, width, kind == 1, e))?;
            if !allowed {
                return Err(if kind == 2 {
                    Fault::IAccViol
                } else {
                    Fault::DAccViol
                });
            }
            if kind != 2
                && (device || s.memory.read_scb(0xd14).unwrap_or(0) & 8 != 0)
                && !(address as usize).is_multiple_of(width)
            {
                return Err(Fault::Unaligned);
            }
        }
        Ok(())
    }
    fn transactional(&self) -> bool {
        !self.0.borrow().functional
    }
    fn observes_core_cycles(&self) -> bool {
        true
    }
    fn begin_instruction(&self, pc: u32, sp: u32, exception: bool) {
        let mut s = self.0.borrow_mut();
        if exception && s.waiting {
            let remaining = s.wait_deadline.saturating_sub(s.now);
            if remaining != 0 {
                s.memory.abandon_wait(remaining);
            }
        }
        let context = ((u64::from(pc) << 32) | u64::from(sp)) ^ if exception { 1 << 63 } else { 0 };
        s.context = Some(context);
        (s.log, s.fetch_log) = s.replay.remove(&context).unwrap_or_default();
        s.cursor = 0;
        s.phase = 0;
        s.waiting = false;
        s.wait_ns = 0;
        s.memory.begin_cpu_block();
    }
    fn instruction_stalled(&self) -> bool {
        self.0.borrow().waiting
    }
    fn can_preempt(&self) -> bool {
        let s = self.0.borrow();
        !s.waiting || s.memory.wait_interruptible
    }
    fn finish_instruction(&self) {
        let mut s = self.0.borrow_mut();
        if let Some(context) = s.context.take() {
            s.replay.remove(&context);
        }
        s.log.clear();
        s.fetch_log.clear();
        s.cursor = 0;
        s.phase = 0;
    }
    fn fetch16_at(&self, a: u32, cycles: u64) -> Result<(u16, bool), Fault> {
        let mut s = self.0.borrow_mut();
        s.synchronize(cycles)
            .map_err(|e| s.failure(a, 2, false, &e.to_string()))?;
        s.fetch(a).map(|v| (v, false))
    }
    fn memory_barrier(&self, kind: u32) -> Result<(), Fault> {
        let mut s = self.0.borrow_mut();
        if s.functional {
            return Ok(());
        }
        if kind == 2 {
            return Ok(());
        }
        match s.memory.barrier(if kind == 0 { 5 } else { 4 }) {
            Ok(0) => Ok(()),
            Ok(ns) => Err(s.stall(ns)),
            Err(e) => Err(s.failure(0, 0, false, e)),
        }
    }
    fn exception_phase(&self, phase: u8, address: u32, bytes: u32) -> Result<(), Fault> {
        let mut s = self.0.borrow_mut();
        if s.functional {
            return Ok(());
        }
        let result = match phase {
            0 => {
                s.phase = 1;
                Ok(0)
            }
            1 => {
                if s.return_wait {
                    let remaining = s.wait_deadline.saturating_sub(s.now);
                    if remaining != 0 {
                        s.memory.abandon_wait(remaining);
                    }
                    s.memory.finish_exception_return();
                    s.return_wait = false;
                }
                s.phase = 2;
                if s.failed_vector == Some(address) {
                    Ok(0)
                } else {
                    s.memory.exception_entry(address)
                }
            }
            2 => {
                s.phase = 0;
                s.failed_vector = None;
                s.memory.finish_exception_entry().map(|()| 0)
            }
            3 => {
                s.phase = 3;
                s.return_wait = true;
                s.memory.exception_return(address, bytes)
            }
            4 => {
                s.phase = 0;
                s.return_wait = false;
                s.memory.finish_exception_return();
                Ok(0)
            }
            _ => Err("invalid exception phase"),
        };
        match result {
            Ok(0) => Ok(()),
            Ok(ns) => Err(s.stall(ns)),
            Err(e) => Err(s.failure(address, 4, false, e)),
        }
    }
    fn core_config_changed(&mut self, a: u32, v: u32) -> Result<(), Fault> {
        let mut s = self.0.borrow_mut();
        let result = if a == 0xe000ed0c {
            if v & 3 != 0 {
                Err("AIRCR debug-only reset/active-clear requests are unsupported")
            } else {
                s.reset_requested = v & 4 != 0;
                Ok(())
            }
        } else if a == 0xe000edfc {
            let now = s.now;
            s.clock.trace_enable(v & (1 << 24) != 0, now)
        } else {
            s.memory.write_scb(u64::from(a - 0xe000e000), v)
        };
        result.map_err(|e| s.failure(a, 4, true, e))
    }
    fn trace_config_changed(&mut self, v: u32) -> Result<(), Fault> {
        self.core_config_changed(0xe000edfc, v)
    }
    fn read32_at(&mut self, a: u32, cycles: u64) -> Result<(u32, bool), Fault> {
        let mut s = self.0.borrow_mut();
        s.synchronize(cycles)
            .map_err(|e| s.failure(a, 4, false, &e.to_string()))?;
        if s.functional {
            return s
                .read(a, 4, true)
                .map(|v| (v, true))
                .map_err(|e| s.failure(a, 4, false, e));
        }
        s.cpu_transfer(a, 4, false, 0)
            .map(|v| (v, !Seed::loadable(a, 4)))
    }
    fn read16_at(&self, a: u32, cycles: u64) -> Result<(u16, bool), Fault> {
        let mut s = self.0.borrow_mut();
        s.synchronize(cycles)
            .map_err(|e| s.failure(a, 2, false, &e.to_string()))?;
        if s.functional {
            return s
                .read(a, 2, true)
                .map(|v| (v as u16, true))
                .map_err(|e| s.failure(a, 2, false, e));
        }
        s.cpu_transfer(a, 2, false, 0)
            .map(|v| (v as u16, !Seed::loadable(a, 2)))
    }
    fn read8_at(&self, a: u32, cycles: u64) -> Result<(u8, bool), Fault> {
        let mut s = self.0.borrow_mut();
        s.synchronize(cycles)
            .map_err(|e| s.failure(a, 1, false, &e.to_string()))?;
        if s.functional {
            return s
                .read(a, 1, true)
                .map(|v| (v as u8, true))
                .map_err(|e| s.failure(a, 1, false, e));
        }
        s.cpu_transfer(a, 1, false, 0)
            .map(|v| (v as u8, !Seed::loadable(a, 1)))
    }
    fn write32_at(&mut self, a: u32, v: u32, cycles: u64) -> Result<bool, Fault> {
        let mut s = self.0.borrow_mut();
        s.synchronize(cycles)
            .map_err(|e| s.failure(a, 4, true, &e.to_string()))?;
        if s.functional {
            return s
                .write(a, 4, v, true)
                .map(|()| true)
                .map_err(|e| s.failure(a, 4, true, e));
        }
        s.cpu_transfer(a, 4, true, v).map(|_| !Seed::loadable(a, 4))
    }
    fn write16_at(&mut self, a: u32, v: u16, cycles: u64) -> Result<bool, Fault> {
        let mut s = self.0.borrow_mut();
        s.synchronize(cycles)
            .map_err(|e| s.failure(a, 2, true, &e.to_string()))?;
        if s.functional {
            return s
                .write(a, 2, u32::from(v), true)
                .map(|()| true)
                .map_err(|e| s.failure(a, 2, true, e));
        }
        s.cpu_transfer(a, 2, true, u32::from(v))
            .map(|_| !Seed::loadable(a, 2))
    }
    fn write8_at(&mut self, a: u32, v: u8, cycles: u64) -> Result<bool, Fault> {
        let mut s = self.0.borrow_mut();
        s.synchronize(cycles)
            .map_err(|e| s.failure(a, 1, true, &e.to_string()))?;
        if s.functional {
            return s
                .write(a, 1, u32::from(v), true)
                .map(|()| true)
                .map_err(|e| s.failure(a, 1, true, e));
        }
        s.cpu_transfer(a, 1, true, u32::from(v))
            .map(|_| !Seed::loadable(a, 1))
    }
    fn read32(&mut self, a: u32) -> Result<u32, Fault> {
        self.0.borrow_mut().cpu_transfer(a, 4, false, 0)
    }
    fn read16(&self, a: u32) -> Result<u16, Fault> {
        self.0
            .borrow_mut()
            .cpu_transfer(a, 2, false, 0)
            .map(|v| v as u16)
    }
    fn read8(&self, a: u32) -> Result<u8, Fault> {
        self.0
            .borrow_mut()
            .cpu_transfer(a, 1, false, 0)
            .map(|v| v as u8)
    }
    fn write32(&mut self, a: u32, v: u32) -> Result<(), Fault> {
        self.0.borrow_mut().cpu_transfer(a, 4, true, v).map(|_| ())
    }
    fn write16(&mut self, a: u32, v: u16) -> Result<(), Fault> {
        self.0
            .borrow_mut()
            .cpu_transfer(a, 2, true, u32::from(v))
            .map(|_| ())
    }
    fn write8(&mut self, a: u32, v: u8) -> Result<(), Fault> {
        self.0
            .borrow_mut()
            .cpu_transfer(a, 1, true, u32::from(v))
            .map(|_| ())
    }
    fn in_range(&self, a: u32) -> bool {
        if self.0.borrow().functional
            && ((0x08000000..0x08020000).contains(&a) || (0x20000000..0x20020000).contains(&a))
        {
            return false;
        }
        self.0.borrow().contains(a)
    }
}
fn rcc_ready(offset: u32, mut value: u32) -> u32 {
    let pairs: &[(u32, u32)] = match offset {
        0 => &[
            (0, 2),
            (7, 8),
            (12, 13),
            (16, 17),
            (24, 25),
            (26, 27),
            (28, 29),
        ],
        0x74 => &[(0, 1)],
        0x70 => &[(0, 1), (8, 9)],
        _ => &[],
    };
    for &(on, ready) in pairs {
        value = (value & !(1 << ready)) | (((value >> on) & 1) << ready);
    }
    if offset == 0x10 {
        value = (value & !0x38) | ((value & 7) << 3);
    }
    value
}

#[cfg(test)]
mod fetch_word_tests {
    use super::*;
    fn device(pc: u32) -> Device {
        let mut seed = Seed::new(Board::Rev5).unwrap();
        seed.load_bytes(pc, &[0xaf, 0xf3, 0x00, 0x80]).unwrap();
        seed.synchronize(0).unwrap();
        Device(Rc::new(RefCell::new(seed)))
    }
    fn fetch(device: &Device, pc: u32, address: u32, cycles: &mut u64) -> u16 {
        for _ in 0..128 {
            match device.fetch16_at(address, *cycles) {
                Ok((value, _)) => return value,
                Err(_) => {
                    assert!(
                        device.instruction_stalled(),
                        "unexpected instruction-fetch fault"
                    );
                    *cycles += 16;
                    device.begin_instruction(pc, 0x2001f000, false);
                }
            }
        }
        panic!("instruction fetch never completed")
    }
    fn completed(device: &Device) -> u64 {
        device
            .0
            .borrow()
            .memory
            .stats()
            .lines()
            .find_map(|line| line.strip_prefix("fabric_completed="))
            .unwrap()
            .parse()
            .unwrap()
    }
    #[test]
    fn thumb32_halves_share_one_service_request_and_hold_the_fetched_word() {
        let pc = 0x08000000;
        let device = device(pc);
        let mut cycles = 0;
        device.begin_instruction(pc, 0x2001f000, false);
        assert_eq!(fetch(&device, pc, pc, &mut cycles), 0xf3af);
        let first = completed(&device);
        assert!(first > 0);
        device
            .0
            .borrow_mut()
            .memory
            .load_flash(pc + 2, 2, 0x1234)
            .unwrap();
        assert_eq!(fetch(&device, pc, pc + 2, &mut cycles), 0x8000);
        assert_eq!(
            completed(&device),
            first,
            "second half requested the same instruction service twice"
        );
        device.finish_instruction();
        device.begin_instruction(pc, 0x2001f000, false);
        assert_eq!(fetch(&device, pc, pc, &mut cycles), 0xf3af);
        assert_eq!(
            fetch(&device, pc, pc + 2, &mut cycles),
            0x1234,
            "a new uncached instruction must see new physical bytes"
        );
        assert_eq!(completed(&device), first + 1);
    }
    #[test]
    fn thumb32_crossing_a_service_line_keeps_the_second_transaction() {
        let pc = 0x0800001e;
        let device = device(pc);
        let mut cycles = 0;
        device.begin_instruction(pc, 0x2001f000, false);
        assert_eq!(fetch(&device, pc, pc, &mut cycles), 0xf3af);
        let first = completed(&device);
        device
            .0
            .borrow_mut()
            .memory
            .load_flash(pc + 2, 2, 0x1234)
            .unwrap();
        assert_eq!(fetch(&device, pc, pc + 2, &mut cycles), 0x1234);
        assert_eq!(completed(&device), first + 1);
    }
}
