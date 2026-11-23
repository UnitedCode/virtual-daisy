//! Scalar DSP multiply and absolute-difference families.
use crate::{
    Processor,
    core::{
        fault::Fault,
        register::{BaseReg, Reg},
    },
    executor::{ExecuteResult, ExecuteSuccess, ExecutorHelper},
};
impl Processor {
    pub(super) fn exec_dsp_multiply(&mut self, opcode: u32) -> ExecuteResult {
        if !self.condition_passed() {
            return Ok(ExecuteSuccess::NotTaken);
        }
        let group = (opcode >> 20) & 15;
        let accumulator = (opcode >> 12) & 15;
        if !cfg!(feature = "has-dsp-ext")
            || opcode & 0xff8000e0 != 0xfb000000
            || !matches!(group, 2..=7)
            || (group == 7 && opcode & 16 != 0)
            || (group == 6 && accumulator == 15)
        {
            return Err(Fault::UndefInstr);
        }
        let rd = Reg::from(((opcode >> 8) & 15) as u8);
        let rn = Reg::from(((opcode >> 16) & 15) as u8);
        let rm = Reg::from((opcode & 15) as u8);
        if [rd, rn, rm].iter().any(|r| matches!(r, Reg::SP | Reg::PC)) || accumulator == 13 {
            return Err(Fault::UndefInstr);
        }
        let a = self.get_r(rn);
        let b = self.get_r(rm);
        let accumulated = accumulator != 15;
        let c = if accumulated {
            self.get_r(Reg::from(accumulator as u8))
        } else {
            0
        };
        let acc = i64::from(c as i32);
        let half = |v: u32| i64::from(v as i16);
        let mut set_q = false;
        let result = match group {
            2 | 4 => {
                let b = if opcode & 16 != 0 {
                    b.rotate_right(16)
                } else {
                    b
                };
                let low = half(a) * half(b);
                let high = half(a >> 16) * half(b >> 16);
                let sum = if group == 2 { low + high } else { low - high } + acc;
                set_q = (group == 2 || accumulated)
                    && (sum < i64::from(i32::MIN) || sum > i64::from(i32::MAX));
                sum as u32
            }
            3 => {
                let product = i64::from(a as i32) * half(b >> (((opcode >> 4) & 1) * 16));
                let sum = (product >> 16) + acc;
                set_q = accumulated && (sum < i64::from(i32::MIN) || sum > i64::from(i32::MAX));
                sum as u32
            }
            5 | 6 => {
                let product = i64::from(a as i32) * i64::from(b as i32);
                let sum = if group == 6 {
                    (acc << 32).wrapping_sub(product)
                } else {
                    (acc << 32).wrapping_add(product)
                };
                let rounding = if opcode & 16 != 0 { 0x80000000 } else { 0 };
                (sum.wrapping_add(rounding) >> 32) as u32
            }
            7 => {
                let difference: u32 = (0..4)
                    .map(|i| u32::from(((a >> (i * 8)) as u8).abs_diff((b >> (i * 8)) as u8)))
                    .sum();
                difference.wrapping_add(c)
            }
            _ => unreachable!(),
        };
        if set_q {
            self.psr.value |= 1 << 27;
        }
        self.set_r(rd, result);
        Ok(ExecuteSuccess::Taken { cycles: 1 })
    }
}
#[cfg(all(test, feature = "has-dsp-ext"))]
mod tests {
    use super::*;
    use crate::{core::thumb::ThumbCode, decoder::Decoder};
    #[test]
    fn typed_halfword_multiply_rejects_invalid_general_registers() {
        for base in [
            0xfb11f002, 0xfb11f012, 0xfb11f022, 0xfb11f032, 0xfb113002, 0xfb113012, 0xfb113022,
            0xfb113032,
        ] {
            for bad in [13u32, 15] {
                for shift in [0, 8, 16] {
                    let mut cpu = Processor::new();
                    cpu.r0_12.fill(0xdeadbeef);
                    let opcode = (base & !(15 << shift)) | (bad << shift);
                    let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                    assert_eq!(cpu.execute_internal(&instruction), Err(Fault::UndefInstr));
                    assert_eq!(cpu.r0_12, [0xdeadbeef; 13]);
                }
            }
        }
    }
    #[test]
    fn invalid_operands_fault_before_any_write() {
        for base in [
            0xfb213002, 0xfb213012, 0xfb21f002, 0xfb313002, 0xfb31f012, 0xfb413002, 0xfb41f012,
            0xfb513002, 0xfb51f012, 0xfb613002, 0xfb613012, 0xfb713002, 0xfb71f002,
        ] {
            for bad in [13u32, 15] {
                for shift in [0, 8, 16] {
                    let mut cpu = Processor::new();
                    cpu.r0_12.fill(0xdeadbeef);
                    cpu.psr.value = 0xa90a0000;
                    let opcode = (base & !(15 << shift)) | (bad << shift);
                    assert_eq!(cpu.exec_dsp_multiply(opcode), Err(Fault::UndefInstr));
                    assert_eq!(cpu.r0_12, [0xdeadbeef; 13]);
                    assert_eq!(cpu.psr.value, 0xa90a0000);
                }
            }
            let mut cpu = Processor::new();
            assert_eq!(
                cpu.exec_dsp_multiply((base & !0xf000) | 0xd000),
                Err(Fault::UndefInstr)
            );
        }
        let mut cpu = Processor::new();
        assert_eq!(cpu.exec_dsp_multiply(0xfb61f002), Err(Fault::UndefInstr));
    }
}
