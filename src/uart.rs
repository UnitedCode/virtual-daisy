//! Baud-paced 8N1 transmitter. Times are virtual nanoseconds, not host time.
use std::collections::VecDeque;

pub struct Transmitter {
    capacity: usize,
    frame_ns: u64,
    pending: VecDeque<(u64, u8)>,
    completed: Vec<(u64, u8)>,
}
impl Transmitter {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            capacity,
            frame_ns: 0,
            pending: VecDeque::new(),
            completed: Vec::new(),
        }
    }
    pub fn configure(&mut self, kernel_hz: u64, divisor: u32) -> Result<(), &'static str> {
        if kernel_hz == 0 || divisor < 16 {
            return Err("invalid oversampling-16 UART clock/divisor");
        }
        let frame = (10_000_000_000u128 * u128::from(divisor)).div_ceil(u128::from(kernel_hz));
        self.frame_ns = u64::try_from(frame).map_err(|_| "UART frame overflow")?;
        Ok(())
    }
    pub fn ready(&mut self, now: u64) -> bool {
        self.advance(now);
        self.pending.len() < self.capacity + 1
    }
    pub fn complete(&mut self, now: u64) -> bool {
        self.advance(now);
        self.pending.is_empty()
    }
    pub fn write(&mut self, byte: u8, now: u64) -> Result<(), &'static str> {
        if self.frame_ns == 0 {
            return Err("UART clock not configured");
        }
        if !self.ready(now) {
            return Err("UART transmit FIFO full");
        }
        let start = self.pending.back().map(|v| v.0).unwrap_or(now);
        self.pending.push_back((
            start
                .checked_add(self.frame_ns)
                .ok_or("UART deadline overflow")?,
            byte,
        ));
        Ok(())
    }
    fn advance(&mut self, now: u64) {
        while self.pending.front().is_some_and(|v| v.0 <= now) {
            self.completed.push(self.pending.pop_front().unwrap());
        }
    }
    pub fn take_completed(&mut self) -> Vec<(u64, u8)> {
        std::mem::take(&mut self.completed)
    }
}
