//! TIM2 up-counting update path used by the firmware's UI scan.
#[derive(Default)]
pub struct Timer {
    regs: [u32; 21],
    ticks: f64,
    psc: u32,
    arr: u32,
    counter: u32,
}
impl Timer {
    pub fn read(&self, offset: u32) -> Result<u32, &'static str> {
        if offset % 4 != 0 || offset > 0x50 {
            return Err("unsupported TIM2 register");
        }
        Ok(match offset {
            0x14 => 0,
            0x24 => self.counter,
            _ => self.regs[offset as usize / 4],
        })
    }
    pub fn write(&mut self, offset: u32, value: u32) -> Result<(), &'static str> {
        self.read(offset)?;
        match offset {
            0 => {
                if value & (7 << 4) != 0 {
                    return Err("TIM2 center/down counting unsupported");
                }
                self.regs[0] = value;
            }
            0x10 => self.regs[4] &= value,
            0x14 => {
                if value & 1 != 0 {
                    self.psc = self.regs[10];
                    self.arr = self.regs[11];
                    self.counter = 0;
                    self.ticks = 0.0;
                    if self.regs[0] & 4 == 0 {
                        self.regs[4] |= 1;
                    }
                }
            }
            0x24 => self.counter = value,
            0x2c => {
                self.regs[11] = value;
                if self.regs[0] & (1 << 7) == 0 {
                    self.arr = value;
                }
            }
            _ => self.regs[offset as usize / 4] = value,
        }
        Ok(())
    }
    pub fn advance(&mut self, ns: u64, hz: f64) {
        if self.regs[0] & 1 == 0 {
            return;
        }
        self.ticks += ns as f64 * hz / 1e9 / f64::from(self.psc + 1);
        let steps = self.ticks as u64;
        self.ticks -= steps as f64;
        let total = u64::from(self.counter) + steps;
        let period = u64::from(self.arr) + 1;
        if total >= period {
            if self.regs[0] & 2 == 0 {
                self.regs[4] |= 1;
            }
            self.psc = self.regs[10];
            self.arr = self.regs[11];
            if self.regs[0] & 8 != 0 {
                self.regs[0] &= !1;
            }
        }
        self.counter = (total % period) as u32;
    }
    pub fn irq(&self) -> bool {
        self.regs[4] & self.regs[3] & 1 != 0
    }
    pub fn next_ns(&self, hz: f64) -> Option<u64> {
        if self.regs[0] & 1 == 0 || hz <= 0.0 {
            return None;
        }
        let ticks = f64::from(self.arr) + 1.0 - f64::from(self.counter) - self.ticks;
        Some(
            (ticks * (f64::from(self.psc) + 1.0) * 1e9 / hz)
                .ceil()
                .max(1.0) as u64,
        )
    }
}
