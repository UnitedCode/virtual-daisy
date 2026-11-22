//! The Seed HAL powers and calibrates ADC1/2 even though Synthphone uses the
//! external audio codec. Conversions are deliberately rejected, not simulated.
use std::collections::BTreeMap;
#[derive(Default)]
pub struct Adc {
    regs: BTreeMap<u32, u32>,
}
impl Adc {
    pub fn read(&self, offset: u32) -> Result<u32, &'static str> {
        let local = offset % 0x100;
        if offset == 0x308 {
            return Ok(self.regs.get(&offset).copied().unwrap_or(0));
        }
        match local {
            0 => Ok(
                if self.regs.get(&(offset + 8)).copied().unwrap_or(1 << 29) & (1 << 28) != 0 {
                    1 << 12
                } else {
                    0
                },
            ),
            8 => Ok(self.regs.get(&offset).copied().unwrap_or(1 << 29)),
            4 | 0xc | 0x10 | 0x14 | 0x18 | 0x20 | 0x30 | 0x34 | 0x38 | 0x3c | 0xc0 | 0xc4
            | 0xc8 => Ok(self.regs.get(&offset).copied().unwrap_or(0)),
            _ => Err("ADC register or conversion not implemented"),
        }
    }
    pub fn write(&mut self, offset: u32, mut value: u32) -> Result<(), &'static str> {
        self.read(offset)?;
        if offset % 0x100 == 8 {
            if value & 5 != 0 {
                return Err("browser ADC supports HAL calibration only, not conversion");
            }
            if value & (1 << 31) != 0 && (value & (1 << 28) == 0 || value & (1 << 29) != 0) {
                return Err("ADC calibration before power up");
            }
            value &= !(1 << 31);
        }
        self.regs.insert(offset, value);
        Ok(())
    }
}
