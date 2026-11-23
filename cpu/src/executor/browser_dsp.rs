//! ARM DSP operations shared by the native and browser hosts.
use crate::{
    Processor,
    core::{
        fault::Fault,
        register::{BaseReg, Reg},
    },
    executor::{ExecuteResult, ExecuteSuccess, ExecutorHelper},
};
impl Processor {
    pub(super) fn exec_browser_dsp(&mut self, opcode: u32) -> ExecuteResult {
        if !self.condition_passed() {
            return Ok(ExecuteSuccess::NotTaken);
        }
        if opcode & 0xfffffff0 == 0xf3bf8f20 {
            self.exclusive_monitor = None;
            return Ok(ExecuteSuccess::Taken { cycles: 1 });
        }
        let rd = Reg::from(((opcode >> 8) & 15) as u8);
        let rn = Reg::from(((opcode >> 16) & 15) as u8);
        let rm = Reg::from((opcode & 15) as u8);
        let a = self.get_r(rn);
        let b = self.get_r(rm);
        let key = opcode & 0xfff0f0f0;
        let result = if opcode & 0xfff0f0c0 == 0xfa80f080 {
            if [rd, rn, rm].iter().any(|r| matches!(r, Reg::SP | Reg::PC)) {
                return Err(Fault::UndefInstr);
            }
            let operation = (opcode >> 4) & 3;
            let mut clipped = false;
            let mut saturate = |value: i64| {
                let result = value.clamp(i64::from(i32::MIN), i64::from(i32::MAX));
                clipped |= result != value;
                result
            };
            // Rm is the first assembly operand, Rn the second. QDADD/QDSUB
            // saturate the doubled second operand before the final operation.
            let second = if operation & 1 != 0 {
                saturate(2 * i64::from(a as i32))
            } else {
                i64::from(a as i32)
            };
            let first = i64::from(b as i32);
            let value = saturate(if operation & 2 != 0 {
                first - second
            } else {
                first + second
            });
            if clipped {
                self.psr.value |= 1 << 27;
            }
            value as u32
        } else if key == 0xfa90f050 || key == 0xfad0f050 {
            if [rd, rn, rm].iter().any(|r| matches!(r, Reg::SP | Reg::PC)) {
                return Err(Fault::UndefInstr);
            }
            // UQADD16/UQSUB16 saturate each unsigned lane without modifying any APSR flag.
            let mut out = 0;
            for shift in [0, 16] {
                let x = ((a >> shift) & 0xffff) as i32;
                let y = ((b >> shift) & 0xffff) as i32;
                let z = if key == 0xfa90f050 { x + y } else { x - y };
                out |= (z.clamp(0, 0xffff) as u32) << shift;
            }
            out
        } else if key == 0xfa80f050 || key == 0xfac0f050 {
            let mut out = 0;
            for lane in 0..4 {
                let x = ((a >> (lane * 8)) & 255) as i32;
                let y = ((b >> (lane * 8)) & 255) as i32;
                let z = if key == 0xfa80f050 { x + y } else { x - y };
                out |= (z.clamp(0, 255) as u32) << (lane * 8);
            }
            out
        } else if opcode & 0xfff08010 == 0xeac00000 {
            let shift = ((opcode >> 12) & 7) * 4 + ((opcode >> 6) & 3);
            if opcode & (1 << 5) == 0 {
                (a & 0xffff) | (b.wrapping_shl(shift) & 0xffff0000)
            } else {
                let shifted = if shift == 0 {
                    ((b as i32) >> 31) as u32
                } else {
                    ((b as i32) >> shift) as u32
                };
                (a & 0xffff0000) | (shifted & 0xffff)
            }
        } else if opcode & 0xfff0f0e0 == 0xfb50f000 {
            let product = i64::from(a as i32) * i64::from(b as i32);
            ((product + if opcode & 16 != 0 { 0x80000000 } else { 0 }) >> 32) as u32
        } else if opcode & 0xfff0f0c0 == 0xfa10f080 {
            a.wrapping_add(b.rotate_right(((opcode >> 4) & 3) * 8) & 0xffff)
        } else if key == 0xfa90f0a0 {
            b.reverse_bits()
        } else {
            return Err(Fault::UndefInstr);
        };
        self.set_r(rd, result);
        Ok(ExecuteSuccess::Taken { cycles: 1 })
    }
}

#[cfg(all(test, feature = "has-dsp-ext"))]
mod word_register_tests {
    use super::*;
    #[test]
    fn invalid_registers_raise_undefined_before_any_write() {
        for base in [0xfa82f081, 0xfa82f0a1, 0xfa82f091, 0xfa82f0b1] {
            for bad in [13u32, 15] {
                for shift in [0, 8, 16] {
                    let opcode = (base & !(15 << shift)) | (bad << shift);
                    let mut cpu = Processor::new();
                    let before = cpu.r0_12;
                    assert_eq!(cpu.exec_browser_dsp(opcode), Err(Fault::UndefInstr));
                    assert_eq!(cpu.r0_12, before);
                }
            }
        }
    }
}
