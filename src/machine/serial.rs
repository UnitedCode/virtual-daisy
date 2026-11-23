use crate::uart::Transmitter;
use std::collections::VecDeque;
pub(super) struct Serial {
    regs: [u32; 12],
    tx: Transmitter,
    rx: VecDeque<u8>,
    incoming: VecDeque<(u64, u8)>,
    tail: u64,
    last: u8,
}
impl Default for Serial {
    fn default() -> Self {
        Self {
            regs: [0; 12],
            tx: Transmitter::new(1),
            rx: VecDeque::new(),
            incoming: VecDeque::new(),
            tail: 0,
            last: 0,
        }
    }
}
impl Serial {
    pub fn reset(&mut self, now: u64) -> Vec<(u64, u8)> {
        self.advance(now);
        let completed = self.take();
        let incoming = std::mem::take(&mut self.incoming);
        let tail = self.tail;
        *self = Self::default();
        self.incoming = incoming;
        self.tail = tail;
        completed
    }
    fn configuration(&mut self, hz: u64) -> Result<(), &'static str> {
        if self.regs[0] & ((1 << 12) | (1 << 28) | (1 << 10) | (1 << 15)) != 0
            || self.regs[1] & 0x3000 != 0
            || self.regs[11] != 0
        {
            return Err("USART1 supports 8N1, oversampling16 and prescaler1");
        }
        self.tx.configure(hz, self.regs[3] & 65535)
    }
    pub fn advance(&mut self, now: u64) {
        while self.incoming.front().is_some_and(|v| v.0 <= now) {
            let (_, byte) = self.incoming.pop_front().unwrap();
            if self.regs[0] & 5 != 5 {
                continue;
            }
            let capacity = if self.regs[0] & (1 << 29) != 0 { 16 } else { 1 };
            if self.rx.len() >= capacity {
                self.regs[7] |= 1 << 3;
            } else {
                self.rx.push_back(byte);
            }
        }
        self.tx.ready(now);
    }
    pub fn receive(&mut self, bytes: &[u8], now: u64, hz: u64) -> Result<(), &'static str> {
        self.configuration(hz)?;
        if self.regs[0] & 5 != 5 {
            return Err("USART1 receiver is not enabled");
        }
        let interval =
            (10_000_000_000u128 * u128::from(self.regs[3] & 65535)).div_ceil(u128::from(hz)) as u64;
        for &byte in bytes {
            self.tail = self
                .tail
                .max(now)
                .checked_add(interval)
                .ok_or("serial time overflow")?;
            self.incoming.push_back((self.tail, byte));
        }
        Ok(())
    }
    pub fn read(&mut self, o: u32, now: u64) -> Result<u32, &'static str> {
        self.advance(now);
        if o > 0x2c || !o.is_multiple_of(4) {
            return Err("unsupported USART1 register");
        }
        Ok(match o {
            0x1c => {
                self.regs[7]
                    | (u32::from(self.tx.ready(now)) << 7)
                    | (u32::from(self.tx.complete(now)) << 6)
                    | if self.regs[0] & 1 != 0 {
                        ((self.regs[0] >> 2) & 1) << 22 | ((self.regs[0] >> 3) & 1) << 21
                    } else {
                        0
                    }
                    | if self.rx.is_empty() { 0 } else { 1 << 5 }
            }
            0x20 => 0,
            0x24 => {
                if let Some(b) = self.rx.pop_front() {
                    self.last = b;
                }
                u32::from(self.last)
            }
            _ => self.regs[o as usize / 4],
        })
    }
    pub fn write(&mut self, o: u32, v: u32, now: u64, hz: u64) -> Result<(), &'static str> {
        if o > 0x2c || !o.is_multiple_of(4) {
            return Err("unsupported USART1 register");
        }
        match o {
            0x1c | 0x24 => return Err("read-only USART1 register"),
            0x20 => self.regs[7] &= !v,
            0x28 => {
                if self.regs[0] & 9 != 9 {
                    return Err("USART1 transmitter is not enabled");
                }
                self.configuration(hz)?;
                self.tx.write(v as u8, now)?;
            }
            0 => {
                if (self.regs[0] ^ v) & (1 << 29) != 0 {
                    self.tx = Transmitter::new(if v & (1 << 29) != 0 { 16 } else { 1 });
                }
                self.regs[0] = v;
            }
            _ => self.regs[o as usize / 4] = v,
        }
        Ok(())
    }
    pub fn irq(&mut self, now: u64) -> bool {
        self.regs[0] & 1 != 0
            && ((!self.rx.is_empty() && self.regs[0] & ((1 << 5) | 4) == ((1 << 5) | 4))
                || (self.tx.ready(now) && self.regs[0] & ((1 << 7) | 8) == ((1 << 7) | 8))
                || (self.tx.complete(now) && self.regs[0] & ((1 << 6) | 8) == ((1 << 6) | 8))
                || (self.regs[7] & (1 << 3) != 0 && self.regs[2] & 1 != 0))
    }
    pub fn next(&self) -> Option<u64> {
        self.incoming
            .front()
            .map(|v| v.0)
            .into_iter()
            .chain(self.tx.next_completion())
            .min()
    }
    pub fn take(&mut self) -> Vec<(u64, u8)> {
        self.tx.take_completed()
    }
}
