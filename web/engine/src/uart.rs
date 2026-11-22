//! Baud-paced USART1 receive path. No host serial devices or sockets required.
use std::collections::VecDeque;
#[derive(Default)]
pub struct Uart {
    regs: [u32; 12],
    pending: VecDeque<(u64, u8)>,
    rx: VecDeque<u8>,
    tail: u64,
    pub transmitted: Vec<u8>,
}
impl Uart {
    pub fn read(&mut self, offset: u32) -> Result<u32, &'static str> {
        if offset % 4 != 0 || offset > 0x2c {
            return Err("unsupported USART1 register");
        }
        Ok(match offset {
            0x1c => {
                self.regs[7]
                    | (1 << 7)
                    | (1 << 6)
                    | if self.regs[0] & 1 != 0 {
                        ((self.regs[0] >> 2) & 1) << 22 | ((self.regs[0] >> 3) & 1) << 21
                    } else {
                        0
                    }
                    | if self.rx.is_empty() { 0 } else { 1 << 5 }
            }
            0x20 => 0,
            0x24 => u32::from(self.rx.pop_front().ok_or("USART1 receive underflow")?),
            _ => self.regs[offset as usize / 4],
        })
    }
    pub fn write(&mut self, offset: u32, value: u32) -> Result<(), &'static str> {
        if offset % 4 != 0 || offset > 0x2c {
            return Err("unsupported USART1 register");
        }
        match offset {
            0x1c | 0x24 => return Err("read-only USART1 register"),
            0x20 => self.regs[7] &= !value,
            0x28 => self.transmitted.push(value as u8),
            _ => self.regs[offset as usize / 4] = value,
        }
        Ok(())
    }
    pub fn receive(&mut self, bytes: &[u8], now: u64, hz: f64) -> Result<(), &'static str> {
        if self.regs[0] & 5 != 5 {
            return Err("USART1 receiver is not ready");
        }
        let divisor = self.regs[3] & 65535;
        if divisor == 0 {
            return Err("USART1 baud rate not configured");
        }
        let baud = hz / f64::from(divisor);
        if (baud - 31250.0).abs() > 31.25 {
            return Err("USART1 is not configured for MIDI baud");
        }
        let interval = (1e10 / baud).round() as u64;
        for &byte in bytes {
            self.tail = self.tail.max(now) + interval;
            self.pending.push_back((self.tail, byte));
        }
        Ok(())
    }
    pub fn advance(&mut self, now: u64) {
        while self.pending.front().is_some_and(|v| v.0 <= now) {
            let (_, v) = self.pending.pop_front().unwrap();
            let capacity = if self.regs[0] & (1 << 29) != 0 { 16 } else { 1 };
            if self.rx.len() >= capacity {
                self.regs[7] |= 1 << 3;
            } else {
                self.rx.push_back(v);
            }
        }
    }
    pub fn irq(&self) -> bool {
        !self.rx.is_empty() && self.regs[0] & (1 << 5) != 0
    }
    pub fn next_event(&self) -> Option<u64> {
        self.pending.front().map(|v| v.0)
    }
}
