//! Blocking STM32 I²C master transactions. Attached devices own their behavior.
#[derive(Default)]
pub(super) struct I2c {
    regs: [u32; 11],
    address: u8,
    remaining: u32,
    packet: Vec<u8>,
}
impl I2c {
    pub fn read(&self, o: u32) -> Result<u32, &'static str> {
        if o > 0x28 || !o.is_multiple_of(4) {
            return Err("unsupported I2C register");
        }
        Ok(self.regs[o as usize / 4])
    }
    pub fn write(&mut self, o: u32, v: u32) -> Result<Option<(u8, Vec<u8>)>, &'static str> {
        self.read(o)?;
        match o {
            0 => {
                self.regs[0] = v;
                if v & 1 == 0 {
                    self.regs[6] = 1;
                }
            }
            4 => {
                self.regs[1] = v & !((1 << 13) | (1 << 14));
                if v & (1 << 14) != 0 {
                    self.regs[6] = 1 | (1 << 5);
                    return Ok(None);
                }
                if v & (1 << 10) != 0 {
                    return Err("I2C receive transactions are not supported yet");
                }
                if v & (1 << 13) != 0 || self.regs[6] & (1 << 7) != 0 {
                    self.address = ((v >> 1) & 127) as u8;
                    self.remaining = (v >> 16) & 255;
                    self.packet.clear();
                    self.regs[6] = if self.remaining > 0 {
                        3 | (1 << 15)
                    } else {
                        1 | (1 << 6)
                    };
                }
            }
            0x1c => self.regs[6] &= !v,
            0x28 => {
                if self.remaining == 0 {
                    return Err("I2C transmit without a transaction");
                }
                self.packet.push(v as u8);
                self.remaining -= 1;
                if self.remaining == 0 {
                    self.regs[6] = if self.regs[1] & (1 << 25) != 0 {
                        1 | (1 << 5)
                    } else if self.regs[1] & (1 << 24) != 0 {
                        1 | (1 << 7)
                    } else {
                        1 | (1 << 6)
                    };
                    return Ok(Some((self.address, std::mem::take(&mut self.packet))));
                }
            }
            _ => self.regs[o as usize / 4] = v,
        }
        Ok(None)
    }
}
