//! MMIO adapter for the existing Virtual Daisy models. Unknown accesses fail.
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};
use virtual_daisy::{memory::SystemMemory, Peripheral};
use zmu_cortex_m::{bus::Bus, core::fault::Fault};

pub struct Board {
    pub error: Option<String>,
    pub reads: u64,
    pub writes: u64,
    pub memory: SystemMemory,
    peripherals: BTreeMap<u32, (u32, Peripheral)>,
    scb: BTreeMap<u32, u32>,
    adc: crate::adc::Adc,
    i2c: [crate::i2c::I2c; 2],
    mux: [u32; 16],
    pub core_rate: f64,
    timer_rate: f64,
    qspi_rate: Option<virtual_daisy::clock::Rate>,
    pub now_ns: u64,
    fraction: f64,
    last_cycles: u64,
    pub input: VecDeque<[f32; 2]>,
    pub dma_beats: u64,
    pub frames: u64,
    timer: crate::timer::Timer,
    uart: crate::uart::Uart,
    keys: u16,
    encoder_events: VecDeque<(u64, u8)>,
    encoder_tail: u64,
    encoder_phase: u8,
    pub mic_gain: f32,
}
impl Board {
    pub fn flash_image(&self) -> Vec<u8> {
        self.peripherals[&0x52005000].1.nonvolatile_image()
    }
    pub fn flash_revision(&self) -> u64 {
        self.peripherals[&0x52005000].1.nonvolatile_revision()
    }
    pub fn restore_flash(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        self.peripherals
            .get_mut(&0x52005000)
            .unwrap()
            .1
            .restore_nonvolatile_image(bytes)
    }
    pub fn new() -> Self {
        let mut peripherals = BTreeMap::new();
        for (base, size, kind) in [
            (0x58024400, 0x400, 13),
            (0x58024800, 0x400, 0),
            (0x52004000, 0x1000, 1),
            (0x58000400, 0x400, 2),
            (0x40020000, 0x400, 6),
            (0x40015800, 0x400, 7),
            (0x52005000, 0x400, 8),
            (0x51000000, 0x1000, 15),
        ] {
            peripherals.insert(base, (size, Peripheral::new(kind)));
        }
        for port in 0..11 {
            let mut gpio = Peripheral::new(100 + port);
            if port == 3 {
                gpio.configure_revision_straps(1).unwrap();
            }
            if port == 2 {
                // The encoder rests at Gray phase 11. Its firmware inputs are
                // floating, so model the contacts' external high level from
                // power-on rather than inventing a first falling edge.
                gpio.drive_pin(8, true).unwrap();
                gpio.drive_pin(9, true).unwrap();
            }
            peripherals.insert(0x58020000 + 0x400 * port, (0x400, gpio));
        }
        let mut memory = SystemMemory::default();
        peripherals
            .get_mut(&0x52005000)
            .unwrap()
            .1
            .attach_memory(&mut memory)
            .unwrap();
        let timer_rate = peripherals[&0x58024400].1.domain_hz(6).unwrap();
        let qspi_rate = peripherals[&0x58024400].1.qspi_clock().unwrap();
        Self {
            error: None,
            reads: 0,
            writes: 0,
            memory,
            peripherals,
            scb: BTreeMap::new(),
            adc: crate::adc::Adc::default(),
            i2c: core::array::from_fn(|_| crate::i2c::I2c::default()),
            mux: [0; 16],
            core_rate: 64_000_000.0,
            timer_rate,
            qspi_rate,
            now_ns: 0,
            fraction: 0.0,
            last_cycles: 0,
            input: VecDeque::new(),
            dma_beats: 0,
            frames: 0,
            timer: crate::timer::Timer::default(),
            uart: crate::uart::Uart::default(),
            keys: 0,
            encoder_events: VecDeque::new(),
            encoder_tail: 0,
            encoder_phase: 3,
            mic_gain: 1.0,
        }
    }
    pub fn read(&mut self, address: u32, width: usize) -> Result<u32, Fault> {
        if [0x58020010, 0x58020410, 0x58020810].contains(&address) {
            self.update_matrix();
        }
        self.reads += 1;
        let result = if SystemMemory::is_ram(address, width)
            || SystemMemory::is_qspi(address, width)
        {
            self.memory.read_cpu(address, width)
        } else if (0x40011000..0x40011030).contains(&address) {
            self.uart.read(address - 0x40011000)
        } else if (0x40000000..0x40000054).contains(&address) {
            self.timer.read(address - 0x40000000)
        } else if (0x52005000..0x52005034).contains(&address) {
            self.peripherals
                .get_mut(&0x52005000)
                .unwrap()
                .1
                .qspi_read(u64::from(address - 0x52005000), width)
        } else if (0x40020800..0x40020840).contains(&address) && width == 4 {
            Ok(self.mux[((address - 0x40020800) / 4) as usize])
        } else if (0x40022000..0x40022400).contains(&address) {
            self.adc.read(address - 0x40022000)
        } else if (0x40005400..0x40005c00).contains(&address) {
            let n = ((address - 0x40005400) / 0x400) as usize;
            self.i2c[n].read((address - 0x40005400) % 0x400)
        } else if (0x52002000..0x52002200).contains(&address) {
            self.memory.flash_read_reg(u64::from(address - 0x52002000))
        } else if (0xe000ed78..=0xe000ed84).contains(&address) {
            self.memory.read_scb(u64::from(address - 0xe000e000))
        } else if address == 0xe000ed90 {
            Ok(16 << 8)
        } else if (0xe000ed94..=0xe000edb8).contains(&address) {
            Ok(self.scb.get(&address).copied().unwrap_or(0))
        } else if let Some((&base, (size, p))) = self.peripherals.range_mut(..=address).next_back()
        {
            if address - base < *size && width == 4 {
                p.read(u64::from(address - base))
            } else {
                Err("unmapped or invalid width")
            }
        } else {
            Err("unmapped")
        };
        result.map_err(|error| {
            self.error = Some(format!("read{width} 0x{address:08x}: {error}"));
            Fault::Preciserr
        })
    }
    pub fn write(&mut self, address: u32, width: usize, value: u32) -> Result<(), Fault> {
        self.writes += 1;
        let result = if SystemMemory::is_ram(address, width) {
            self.memory.write_cpu(address, width, value)
        } else if (0x40011000..0x40011030).contains(&address) {
            self.uart.write(address - 0x40011000, value)
        } else if (0x40000000..0x40000054).contains(&address) {
            self.timer.write(address - 0x40000000, value)
        } else if (0x52005000..0x52005034).contains(&address) {
            self.peripherals.get_mut(&0x52005000).unwrap().1.qspi_write(
                u64::from(address - 0x52005000),
                width,
                value,
            )
        } else if (0x40020800..0x40020840).contains(&address) && width == 4 {
            if value & !127 != 0 {
                Err("DMAMUX synchronization not implemented")
            } else {
                self.mux[((address - 0x40020800) / 4) as usize] = value;
                Ok(())
            }
        } else if (0x40022000..0x40022400).contains(&address) {
            self.adc.write(address - 0x40022000, value)
        } else if (0x40005400..0x40005c00).contains(&address) {
            let n = ((address - 0x40005400) / 0x400) as usize;
            self.i2c[n].write((address - 0x40005400) % 0x400, value)
        } else if (0x52002000..0x52002200).contains(&address) {
            self.memory
                .flash_write_reg(u64::from(address - 0x52002000), value, width)
        } else if (0xe000ed78..=0xe000edb8).contains(&address)
            || (0xe000ef50..=0xe000ef78).contains(&address)
        {
            self.scb.insert(address, value);
            self.memory
                .write_scb(u64::from(address - 0xe000e000), value)
        } else if let Some((&base, (size, p))) = self.peripherals.range_mut(..=address).next_back()
        {
            if address - base < *size && width == 4 {
                let offset = u64::from(address - base);
                let value = if base == 0x58024400 {
                    rcc_ready(offset, value)
                } else {
                    value
                };
                p.write(offset, value)
            } else {
                Err("unmapped or invalid width")
            }
        } else {
            Err("unmapped")
        };
        let result = result.and_then(|()| {
            if (0x58024400..0x58024800).contains(&address) {
                self.core_rate = self.peripherals[&0x58024400].1.core_hz()?;
                self.timer_rate = self.peripherals[&0x58024400].1.domain_hz(6)?;
                self.qspi_rate = self.peripherals[&0x58024400].1.qspi_clock()?;
            }
            if (0x40020000..0x40020400).contains(&address)
                || (0x40015800..0x40015848).contains(&address)
            {
                self.pump_dma()?;
            }
            Ok(())
        });
        result.map_err(|error| {
            self.error = Some(format!(
                "write{width} 0x{address:08x} = 0x{value:08x}: {error}"
            ));
            Fault::Preciserr
        })
    }
    pub fn pump_dma(&mut self) -> Result<(), &'static str> {
        for _ in 0..32 {
            let lines = self.peripherals[&0x40015800].1.audio_requests();
            let mut requests = 0;
            for s in 0..8 {
                if (self.mux[s] == 87 && lines & 1 != 0) || (self.mux[s] == 88 && lines & 2 != 0) {
                    requests |= 1 << s;
                }
            }
            let Some((stream, source, destination)) = self
                .peripherals
                .get_mut(&0x40020000)
                .unwrap()
                .1
                .dma_transfer(requests)?
            else {
                return Ok(());
            };
            let word = if SystemMemory::is_ram(source, 4) {
                self.memory.read_dma(source, 4)?
            } else {
                self.peripherals
                    .get_mut(&0x40015800)
                    .unwrap()
                    .1
                    .read(u64::from(source - 0x40015800))?
            };
            if SystemMemory::is_ram(destination, 4) {
                self.memory.write_dma(destination, 4, word)?;
            } else {
                self.peripherals
                    .get_mut(&0x40015800)
                    .unwrap()
                    .1
                    .write(u64::from(destination - 0x40015800), word)?;
            }
            let done = self
                .peripherals
                .get_mut(&0x40020000)
                .unwrap()
                .1
                .dma_complete(stream)?;
            if done != Some((source, destination)) {
                return Err("DMA descriptor changed during its transfer");
            }
            self.dma_beats += 1;
        }
        Err("unbounded SAI DMA request")
    }
    pub fn synchronize(&mut self, cycles: u64) -> Result<(), &'static str> {
        let delta = cycles
            .checked_sub(self.last_cycles)
            .ok_or("core clock went backwards")?;
        self.last_cycles = cycles;
        self.advance(delta)
    }
    fn advance(&mut self, cycles: u64) -> Result<(), &'static str> {
        self.fraction += cycles as f64 * 1e9 / self.core_rate;
        let ns = self.fraction as u64;
        self.fraction -= ns as f64;
        self.now_ns += ns;
        if ns == 0 {
            return Ok(());
        }
        while self
            .encoder_events
            .front()
            .is_some_and(|v| v.0 <= self.now_ns)
        {
            let (_, phase) = self.encoder_events.pop_front().unwrap();
            self.peripherals
                .get_mut(&0x58020800)
                .unwrap()
                .1
                .drive_pin(9, phase & 2 != 0)?;
            self.peripherals
                .get_mut(&0x58020800)
                .unwrap()
                .1
                .drive_pin(8, phase & 1 != 0)?;
        }
        self.uart.advance(self.now_ns);
        self.timer.advance(ns, self.timer_rate);
        let rate = self.qspi_rate;
        self.peripherals
            .get_mut(&0x52005000)
            .unwrap()
            .1
            .advance_qspi(self.now_ns, rate)?;
        let sai = &mut self.peripherals.get_mut(&0x40015800).unwrap().1;
        let cr = sai.read(4)?;
        let frame = sai.read(0xc)?;
        let frames = self
            .peripherals
            .get_mut(&0x58024400)
            .unwrap()
            .1
            .audio_clock(cr, frame, ns)?;
        for _ in 0..frames {
            let mask = self.i2c[1].codec.read(0x100)?;
            let sample = self
                .input
                .pop_front()
                .unwrap_or([0.0; 2])
                .map(|s| s * self.mic_gain);
            self.peripherals
                .get_mut(&0x40015800)
                .unwrap()
                .1
                .audio_frame(mask, sample)?;
            self.pump_dma()?;
            self.frames += 1;
        }
        Ok(())
    }
    pub fn next_service_cycles(&mut self, maximum_ns: u64) -> Result<u64, &'static str> {
        let sai = &mut self.peripherals.get_mut(&0x40015800).unwrap().1;
        let cr = sai.read(4)?;
        let frame = sai.read(0xc)?;
        let mut ns = maximum_ns;
        if let Some(next) = self.peripherals[&0x58024400].1.next_audio_ns(cr, frame)? {
            ns = ns.min(next);
        }
        if let Some(next) = self.timer.next_ns(self.timer_rate) {
            ns = ns.min(next);
        }
        for next in [
            self.uart.next_event(),
            self.encoder_events.front().map(|v| v.0),
            self.peripherals[&0x52005000].1.next_qspi_event(),
        ]
        .into_iter()
        .flatten()
        {
            ns = ns.min(next.saturating_sub(self.now_ns).max(1));
        }
        Ok(((ns as f64 - self.fraction) * self.core_rate / 1e9)
            .ceil()
            .max(1.0) as u64)
    }
    fn synchronize_access(
        &mut self,
        address: u32,
        width: usize,
        cycles: u64,
    ) -> Result<bool, Fault> {
        let boundary = !SystemMemory::is_ram(address, width);
        if boundary {
            self.synchronize(cycles).map_err(|error| {
                self.error = Some(error.into());
                Fault::Preciserr
            })?;
        }
        Ok(boundary)
    }
    pub fn uart_irq(&self) -> bool {
        self.uart.irq()
    }
    pub fn key(&mut self, key: u8, pressed: bool) -> Result<(), &'static str> {
        if key >= 12 {
            return Err("key index must be 0..11");
        }
        if pressed {
            self.keys |= 1 << key;
        } else {
            self.keys &= !(1 << key);
        }
        self.update_matrix();
        Ok(())
    }
    fn update_matrix(&mut self) {
        let columns = [(0x58020800, 4), (0x58020800, 1), (0x58020000, 6)];
        let mut low = [false; 3];
        for (i, (base, pin)) in columns.iter().enumerate() {
            low[i] = self
                .peripherals
                .get_mut(base)
                .unwrap()
                .1
                .read(0x10)
                .unwrap()
                & (1 << pin)
                == 0;
        }
        for (row, (base, pin)) in [
            (0x58020800, 0),
            (0x58020000, 3),
            (0x58020400, 1),
            (0x58020000, 7),
        ]
        .iter()
        .enumerate()
        {
            let held = (0..3).any(|c| self.keys & (1 << (row * 3 + 2 - c)) != 0 && low[c]);
            self.peripherals
                .get_mut(base)
                .unwrap()
                .1
                .drive_pin(*pin, !held)
                .unwrap();
        }
    }
    pub fn switch(&mut self, pin: u32, high: bool) -> Result<(), &'static str> {
        if ![10, 11].contains(&pin) {
            return Err("unsupported phone switch");
        }
        self.peripherals
            .get_mut(&0x58020800)
            .unwrap()
            .1
            .drive_pin(pin, high)
    }
    pub fn turn(&mut self, steps: i32) {
        for _ in 0..steps.unsigned_abs().min(32) {
            for _ in 0..4 {
                self.encoder_phase = if steps > 0 {
                    match self.encoder_phase {
                        3 => 2,
                        2 => 0,
                        0 => 1,
                        _ => 3,
                    }
                } else {
                    match self.encoder_phase {
                        3 => 1,
                        1 => 0,
                        0 => 2,
                        _ => 3,
                    }
                };
                self.encoder_tail = self.encoder_tail.max(self.now_ns) + 2_000_000;
                self.encoder_events
                    .push_back((self.encoder_tail, self.encoder_phase));
            }
        }
    }
    pub fn oled(&self) -> Vec<u8> {
        self.i2c[0].oled.bytes[..512].to_vec()
    }
    pub fn receive_midi(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        self.uart.receive(
            bytes,
            self.now_ns,
            self.peripherals[&0x58024400].1.domain_hz(3)?,
        )
    }
    pub fn timer_irq(&self) -> bool {
        self.timer.irq()
    }
    pub fn irqs(&self) -> u32 {
        self.peripherals[&0x40020000].1.dma_interrupts()
    }
    pub fn take_audio(&mut self) -> Vec<f32> {
        self.peripherals
            .get_mut(&0x40015800)
            .unwrap()
            .1
            .take_audio()
            .unwrap()
            .into_iter()
            .map(|f| f[0])
            .collect()
    }
    pub fn audio_status(&mut self) -> String {
        let a = self
            .peripherals
            .get_mut(&0x40015800)
            .unwrap()
            .1
            .read(4)
            .unwrap();
        let b = self
            .peripherals
            .get_mut(&0x40015800)
            .unwrap()
            .1
            .read(0x24)
            .unwrap();
        let fr = self
            .peripherals
            .get_mut(&0x40015800)
            .unwrap()
            .1
            .read(0xc)
            .unwrap();
        let gate = self
            .peripherals
            .get_mut(&0x58024400)
            .unwrap()
            .1
            .read(0xf0)
            .unwrap();
        let codec = self.i2c[1].codec.read(0x100);
        format!(
            "SAI_A={a:08x} B={b:08x} FRCR={fr:08x} RCC={gate:08x} codec={codec:?} OLED={}",
            self.i2c[0].oled.writes
        )
    }
    pub fn sample_rate(&mut self) -> Result<f64, &'static str> {
        let sai = &mut self.peripherals.get_mut(&0x40015800).unwrap().1;
        let cr = sai.read(4)?;
        let frame = sai.read(0xc)?;
        self.peripherals[&0x58024400].1.audio_hz(cr, frame)
    }
}

// Functional oscillator service (instant readiness, as in the Renode adapter).
// Oscillator stabilization delays are not claimed by this backend.
fn rcc_ready(offset: u64, mut value: u32) -> u32 {
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
        0x74 | 0x78 => &[(0, 1)],
        _ => &[],
    };
    for &(enable, ready) in pairs {
        value = (value & !(1 << ready)) | (((value >> enable) & 1) << ready);
    }
    if offset == 0x10 {
        value = (value & !0x38) | ((value & 7) << 3);
    }
    value
}
pub struct Device(pub Rc<RefCell<Board>>);
impl Bus for Device {
    fn observes_core_cycles(&self) -> bool {
        true
    }
    fn read32_at(&mut self, addr: u32, cycles: u64) -> Result<(u32, bool), Fault> {
        let mut board = self.0.borrow_mut();
        let boundary = board.synchronize_access(addr, 4, cycles)?;
        board.read(addr, 4).map(|value| (value, boundary))
    }
    fn read16_at(&self, addr: u32, cycles: u64) -> Result<(u16, bool), Fault> {
        let mut board = self.0.borrow_mut();
        let boundary = board.synchronize_access(addr, 2, cycles)?;
        board.read(addr, 2).map(|value| (value as u16, boundary))
    }
    fn read8_at(&self, addr: u32, cycles: u64) -> Result<(u8, bool), Fault> {
        let mut board = self.0.borrow_mut();
        let boundary = board.synchronize_access(addr, 1, cycles)?;
        board.read(addr, 1).map(|value| (value as u8, boundary))
    }
    fn write32_at(&mut self, addr: u32, value: u32, cycles: u64) -> Result<bool, Fault> {
        let mut board = self.0.borrow_mut();
        let boundary = board.synchronize_access(addr, 4, cycles)?;
        board.write(addr, 4, value).map(|()| boundary)
    }
    fn write16_at(&mut self, addr: u32, value: u16, cycles: u64) -> Result<bool, Fault> {
        let mut board = self.0.borrow_mut();
        let boundary = board.synchronize_access(addr, 2, cycles)?;
        board.write(addr, 2, u32::from(value)).map(|()| boundary)
    }
    fn write8_at(&mut self, addr: u32, value: u8, cycles: u64) -> Result<bool, Fault> {
        let mut board = self.0.borrow_mut();
        let boundary = board.synchronize_access(addr, 1, cycles)?;
        board.write(addr, 1, u32::from(value)).map(|()| boundary)
    }
    fn peek_ordinary32(&self, address: u32) -> Option<u32> {
        self.0.borrow().memory.peek_cpu32(address)
    }
    fn can_write_ordinary32(&self, address: u32) -> bool {
        self.0.borrow().memory.can_write_cached32(address)
    }
    fn core_config_changed(&mut self, address: u32, value: u32) -> Result<(), Fault> {
        self.0
            .borrow_mut()
            .memory
            .write_scb(u64::from(address - 0xe000e000), value)
            .map_err(|_| Fault::Preciserr)
    }
    fn read32(&mut self, addr: u32) -> Result<u32, Fault> {
        self.0.borrow_mut().read(addr, 4)
    }
    fn read16(&self, addr: u32) -> Result<u16, Fault> {
        self.0.borrow_mut().read(addr, 2).map(|v| v as u16)
    }
    fn read8(&self, addr: u32) -> Result<u8, Fault> {
        self.0.borrow_mut().read(addr, 1).map(|v| v as u8)
    }
    fn write32(&mut self, addr: u32, v: u32) -> Result<(), Fault> {
        self.0.borrow_mut().write(addr, 4, v)
    }
    fn write16(&mut self, addr: u32, v: u16) -> Result<(), Fault> {
        self.0.borrow_mut().write(addr, 2, v as u32)
    }
    fn write8(&mut self, addr: u32, v: u8) -> Result<(), Fault> {
        self.0.borrow_mut().write(addr, 1, v as u32)
    }
    fn in_range(&self, addr: u32) -> bool {
        SystemMemory::is_ram(addr, 1)
            || SystemMemory::is_qspi(addr, 1)
            || (0x40000000..0x60000000).contains(&addr)
            || (0xe000ed78..=0xe000ed84).contains(&addr)
            || (0xe000ed90..=0xe000edb8).contains(&addr)
            || (0xe000ef50..=0xe000ef78).contains(&addr)
    }
}
