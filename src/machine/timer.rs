//! TIM2 internal-clock up-counter and update interrupt.
pub struct Timer {
    regs: [u32; 21],
    // Kernel-clock cycles left in the prescaler, before division by PSC+1.
    ticks: f64,
    psc: u32,
    arr: u32,
    counter: u32,
}
impl Default for Timer {
    fn default() -> Self {
        let mut regs = [0; 21];
        regs[11] = u32::MAX;
        Self {
            regs,
            ticks: 0.,
            psc: 0,
            arr: u32::MAX,
            counter: 0,
        }
    }
}
impl Timer {
    pub fn read(&self, offset: u32) -> Result<u32, &'static str> {
        if !offset.is_multiple_of(4) || offset > 0x50 {
            return Err("unsupported TIM2 register");
        }
        Ok(match offset {
            0x14 => 0,
            0x24 if self.regs[0] & (1 << 11) != 0 => {
                (self.counter & 0x7fff_ffff) | ((self.regs[4] & 1) << 31)
            }
            0x24 => self.counter,
            _ => self.regs[offset as usize / 4],
        })
    }
    pub fn write(&mut self, offset: u32, value: u32) -> Result<(), &'static str> {
        self.read(offset)?;
        if matches!(offset, 4 | 8 | 0x18 | 0x1c | 0x20) && value != 0 {
            return Err("TIM2 trigger/capture/output modes are unsupported");
        }
        if offset == 0xc && value & !1 != 0 {
            return Err("TIM2 only supports update interrupts");
        }
        match offset {
            0 => {
                if value & (7 << 4) != 0 {
                    return Err("TIM2 center/down counting unsupported");
                }
                self.regs[0] = value & 0xb8f;
            }
            0x10 => self.regs[4] &= value,
            0x14 => {
                if value & !1 != 0 {
                    return Err("TIM2 only supports software update events");
                }
                if value & 1 != 0 {
                    self.counter = 0;
                    self.ticks = 0.0;
                    self.update(false);
                }
            }
            0x24 => self.counter = value,
            0x28 => self.regs[10] = value & 0xffff,
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
    fn update(&mut self, overflow: bool) {
        if self.regs[0] & 2 != 0 {
            return;
        }
        self.psc = self.regs[10];
        self.arr = self.regs[11];
        if overflow || self.regs[0] & 4 == 0 {
            self.regs[4] |= 1;
        }
        if self.regs[0] & 8 != 0 {
            self.regs[0] &= !1;
        }
    }
    fn until_overflow(&self) -> u64 {
        // CNT can be written above ARR. It then wraps at the counter's width.
        u64::from(if self.counter <= self.arr {
            self.arr
        } else {
            u32::MAX
        }) + 1
            - u64::from(self.counter)
    }
    pub fn advance(&mut self, ns: u64, hz: f64) {
        if self.regs[0] & 1 == 0 || self.arr == 0 || hz <= 0.0 {
            return;
        }
        self.ticks += ns as f64 * hz / 1e9;
        let divider = f64::from(self.psc) + 1.;
        let steps = (self.ticks / divider) as u64;
        let distance = self.until_overflow();
        if steps < distance {
            self.ticks -= steps as f64 * divider;
            self.counter = self.counter.wrapping_add(steps as u32);
            return;
        }
        self.ticks -= distance as f64 * divider;
        self.counter = 0;
        self.update(true);
        if self.regs[0] & 1 == 0 || self.arr == 0 {
            self.ticks = 0.;
            return;
        }
        // After the first boundary, PSC/ARR are stable until the next MMIO
        // write. Batch further wraps without iterating over every clock edge.
        let divider = f64::from(self.psc) + 1.;
        let steps = (self.ticks / divider) as u64;
        self.ticks -= steps as f64 * divider;
        self.counter = (steps % (u64::from(self.arr) + 1)) as u32;
    }
    pub fn irq(&self) -> bool {
        self.regs[4] & self.regs[3] & 1 != 0
    }
    pub fn next_ns(&self, hz: f64) -> Option<u64> {
        if self.regs[0] & 1 == 0 || self.arr == 0 || hz <= 0.0 {
            return None;
        }
        let ticks = self.until_overflow() as f64 * (f64::from(self.psc) + 1.0) - self.ticks;
        Some((ticks * 1e9 / hz).ceil().max(1.0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::Timer;
    fn configured(cr1: u32) -> Timer {
        let mut t = Timer::default();
        t.write(0x2c, 9).unwrap();
        t.write(0x14, 1).unwrap();
        t.write(0x10, 0).unwrap();
        t.write(0, cr1).unwrap();
        t
    }
    #[test]
    fn disabled_update_keeps_preloads_and_ug_still_reinitializes_counter() {
        let mut t = configured(1 | 2 | 128);
        t.write(0x28, 1).unwrap();
        t.write(0x2c, 19).unwrap();
        t.advance(5000, 1_000_000.);
        assert_eq!(t.read(0x24).unwrap(), 5);
        t.write(0x14, 1).unwrap();
        assert_eq!(t.read(0x24).unwrap(), 0);
        assert_eq!(t.read(0x10).unwrap(), 0, "UDIS suppresses software UIF");
        t.advance(15_000, 1_000_000.);
        assert_eq!(t.read(0x24).unwrap(), 5, "UDIS retains old PSC/ARR");
        assert_eq!(t.read(0x10).unwrap(), 0);
    }
    #[test]
    fn overflow_loads_new_prescaler_and_reload_at_the_boundary() {
        let mut t = configured(1 | 128);
        t.write(0x28, 1).unwrap();
        t.write(0x2c, 19).unwrap();
        t.advance(15_000, 1_000_000.);
        assert_eq!(t.read(0x24).unwrap(), 2);
        assert_eq!(t.next_ns(1_000_000.), Some(35_000));
        t.advance(35_000, 1_000_000.);
        assert_eq!(t.read(0x24).unwrap(), 0);
    }
    #[test]
    fn one_pulse_stops_at_first_update_without_counting_remaining_time() {
        let mut t = configured(1 | 8);
        t.advance(35_000, 1_000_000.);
        assert_eq!(t.read(0x24).unwrap(), 0);
        assert_eq!(t.read(0).unwrap() & 1, 0);
        assert_eq!(t.read(0x10).unwrap(), 1);
    }
    #[test]
    fn disabled_updates_do_not_stop_one_pulse() {
        let mut t = configured(1 | 2 | 8);
        t.advance(35_000, 1_000_000.);
        assert_eq!(t.read(0x24).unwrap(), 5);
        assert_eq!(t.read(0).unwrap() & 1, 1);
    }
    #[test]
    fn software_update_source_masks_irq_without_blocking_shadow_loads() {
        let mut t = configured(4 | 128);
        t.write(0x28, 1).unwrap();
        t.write(0x2c, 19).unwrap();
        t.write(0x14, 1).unwrap();
        assert_eq!(t.read(0x10).unwrap(), 0);
        t.write(0, 1 | 4 | 128).unwrap();
        t.advance(39_000, 1_000_000.);
        assert_eq!(t.read(0x24).unwrap(), 19);
        assert_eq!(t.read(0x10).unwrap(), 0);
        t.advance(1000, 1_000_000.);
        assert_eq!(t.read(0x10).unwrap(), 1);
    }
    #[test]
    fn null_reload_blocks_counting_and_uif_remap_copies_the_flag() {
        let mut t = configured(1 | 2048);
        t.write(0x14, 1).unwrap();
        assert_eq!(t.read(0x24).unwrap(), 1 << 31);
        t.write(0x10, 0).unwrap();
        t.write(0x2c, 0).unwrap();
        t.advance(100_000, 1_000_000.);
        assert_eq!(t.read(0x24).unwrap(), 0);
        assert_eq!(t.next_ns(1_000_000.), None);
    }
    #[test]
    fn batching_does_not_move_a_pending_prescaler_change() {
        let mut whole = configured(1 | 128);
        let mut split = configured(1 | 128);
        for t in [&mut whole, &mut split] {
            t.write(0x28, 2).unwrap();
            t.write(0x2c, 4).unwrap();
        }
        whole.advance(61_000, 1_000_000.);
        for _ in 0..61 {
            split.advance(1000, 1_000_000.);
        }
        assert_eq!(whole.read(0x24).unwrap(), split.read(0x24).unwrap());
        assert_eq!(whole.next_ns(1_000_000.), split.next_ns(1_000_000.));
    }
}
