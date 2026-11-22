//! STM32H7 blocking I2C transmit service for codec control and SSD1306.
use virtual_daisy::Peripheral;
pub struct I2c {
    regs: [u32; 11],
    address: u8,
    remaining: u32,
    packet: Vec<u8>,
    pub codec: Peripheral,
    pub oled: Oled,
}
impl Default for I2c {
    fn default() -> Self {
        Self {
            regs: [0; 11],
            address: 0,
            remaining: 0,
            packet: Vec::new(),
            codec: Peripheral::new(4),
            oled: Oled::default(),
        }
    }
}
impl I2c {
    pub fn read(&self, offset: u32) -> Result<u32, &'static str> {
        match offset {
            0 | 4 | 8 | 0xc | 0x10 | 0x14 | 0x18 | 0x1c | 0x20 | 0x24 | 0x28 => {
                Ok(self.regs[offset as usize / 4])
            }
            _ => Err("unsupported I2C register"),
        }
    }
    pub fn write(&mut self, offset: u32, value: u32) -> Result<(), &'static str> {
        self.read(offset)?;
        match offset {
            0 => {
                self.regs[0] = value;
                if value & 1 == 0 {
                    self.regs[6] = 1;
                }
            }
            4 => {
                self.regs[1] = value & !((1 << 13) | (1 << 14));
                if value & (1 << 14) != 0 {
                    self.regs[6] = 1 | (1 << 5);
                    return Ok(());
                }
                if value & (1 << 10) != 0 {
                    return Err("I2C slave reads are unsupported");
                }
                if value & (1 << 13) != 0 || self.regs[6] & (1 << 7) != 0 {
                    self.address = ((value >> 1) & 127) as u8;
                    if ![0x1a, 0x3c].contains(&self.address) {
                        return Err("unknown I2C slave");
                    }
                    self.remaining = (value >> 16) & 255;
                    self.packet.clear();
                    self.regs[6] = if self.remaining > 0 {
                        3 | (1 << 15)
                    } else {
                        1 | (1 << 6)
                    };
                }
            }
            0x1c => self.regs[6] &= !value,
            0x28 => {
                if self.remaining == 0 {
                    return Err("I2C transmit without transfer");
                }
                self.packet.push(value as u8);
                self.remaining -= 1;
                if self.remaining == 0 {
                    if self.address == 0x1a {
                        self.codec.i2c_write(&self.packet)?;
                    } else {
                        self.oled.write(&self.packet)?;
                    }
                    self.regs[6] = 1 | if self.regs[1] & (1 << 25) != 0 {
                        1 << 5
                    } else if self.regs[1] & (1 << 24) != 0 {
                        (1 << 7) | (1 << 15)
                    } else {
                        (1 << 6) | (1 << 15)
                    };
                }
            }
            _ => self.regs[offset as usize / 4] = value,
        }
        Ok(())
    }
}
pub struct Oled {
    pub bytes: [u8; 1024],
    pub writes: u64,
    page: usize,
    column: usize,
    column_start: usize,
    column_end: usize,
    page_start: usize,
    page_end: usize,
    pending: Option<(u8, Vec<u8>)>,
    mode: u8,
}
impl Default for Oled {
    fn default() -> Self {
        Self {
            bytes: [0; 1024],
            writes: 0,
            page: 0,
            column: 0,
            column_start: 0,
            column_end: 127,
            page_start: 0,
            page_end: 7,
            pending: None,
            mode: 2,
        }
    }
}
impl Oled {
    fn command(&mut self, value: u8) {
        if let Some((cmd, mut params)) = self.pending.take() {
            params.push(value);
            let count = if cmd == 0x21 || cmd == 0x22 { 2 } else { 1 };
            if params.len() < count {
                self.pending = Some((cmd, params));
                return;
            }
            match cmd {
                0x20 => self.mode = params[0],
                0x21 => {
                    self.column_start = (params[0] & 127) as usize;
                    self.column_end = (params[1] & 127) as usize;
                    self.column = self.column_start;
                }
                0x22 => {
                    self.page_start = (params[0] & 7) as usize;
                    self.page_end = (params[1] & 7) as usize;
                    self.page = self.page_start;
                }
                _ => {}
            }
        } else {
            match value {
                0x00..=0x0f => self.column = (self.column & 0xf0) | (value as usize),
                0x10..=0x1f => self.column = (self.column & 15) | (((value & 15) as usize) << 4),
                0xb0..=0xb7 => self.page = (value & 7) as usize,
                0x20 | 0x21 | 0x22 | 0x81 | 0x8d | 0xa8 | 0xd3 | 0xd5 | 0xd9 | 0xda | 0xdb => {
                    self.pending = Some((value, Vec::new()))
                }
                _ => {}
            }
        }
    }
    pub fn write(&mut self, packet: &[u8]) -> Result<(), &'static str> {
        let Some((&control, data)) = packet.split_first() else {
            return Err("empty OLED packet");
        };
        match control {
            0 | 0x80 => {
                for &v in data {
                    self.command(v)
                }
            }
            0x40 => {
                for &v in data {
                    if self.column < 128 && self.page < 8 {
                        self.bytes[self.page * 128 + self.column] = v;
                    }
                    self.column += 1;
                    if self.column > self.column_end {
                        self.column = self.column_start;
                        if self.mode == 0 {
                            self.page += 1;
                            if self.page > self.page_end {
                                self.page = self.page_start;
                            }
                        }
                    }
                }
                self.writes += 1;
            }
            _ => return Err("unsupported OLED control byte"),
        }
        Ok(())
    }
}
