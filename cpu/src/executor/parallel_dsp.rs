//! Packed DSP add/subtract, exchange, saturation and halving.
use crate::{
    Processor,
    core::{
        fault::Fault,
        register::{BaseReg, Reg},
    },
    executor::{ExecuteResult, ExecuteSuccess, ExecutorHelper},
};
impl Processor {
    pub(super) fn exec_parallel_dsp(&mut self, opcode: u32) -> ExecuteResult {
        if !self.condition_passed() {
            return Ok(ExecuteSuccess::NotTaken);
        }
        if !cfg!(feature = "has-dsp-ext") {
            return Err(Fault::UndefInstr);
        }
        let operation = (opcode >> 20) & 15;
        let flavor = (opcode >> 4) & 7;
        if opcode & 0xff80f080 != 0xfa80f000
            || !matches!(operation, 8 | 9 | 10 | 12 | 13 | 14)
            || !matches!(flavor, 0 | 1 | 2 | 4 | 5 | 6)
        {
            return Err(Fault::UndefInstr);
        }
        let rd = Reg::from(((opcode >> 8) & 15) as u8);
        let rn = Reg::from(((opcode >> 16) & 15) as u8);
        let rm = Reg::from((opcode & 15) as u8);
        if [rd, rn, rm].iter().any(|r| matches!(r, Reg::SP | Reg::PC)) {
            return Err(Fault::UndefInstr);
        }
        let a = self.get_r(rn);
        let b = self.get_r(rm);
        let signed = flavor < 4;
        let mode = flavor & 3;
        // Normal forms replace GE. Packed saturating/halving forms preserve
        // every APSR field, including Q; they differ from scalar QADD/QSUB.
        let width = if matches!(operation, 8 | 12) { 8 } else { 16 };
        let mask = (1u32 << width) - 1;
        let mut result = 0;
        let mut ge = 0;
        for lane in 0..32 / width {
            let exchanged = matches!(operation, 10 | 14);
            let other = if exchanged { 1 - lane } else { lane };
            let x = (a >> (lane * width)) & mask;
            let y = (b >> (other * width)) & mask;
            let extend = |v: u32| {
                if signed {
                    i64::from((v << (32 - width)) as i32) >> (32 - width)
                } else {
                    i64::from(v)
                }
            };
            let subtract = matches!(operation, 12 | 13)
                || (operation == 10 && lane == 0)
                || (operation == 14 && lane == 1);
            let raw = if subtract {
                extend(x) - extend(y)
            } else {
                extend(x) + extend(y)
            };
            let value = match mode {
                1 => {
                    if signed {
                        raw.clamp(-(1i64 << (width - 1)), (1i64 << (width - 1)) - 1)
                    } else {
                        raw.clamp(0, i64::from(mask))
                    }
                }
                2 => raw >> 1,
                _ => raw,
            };
            result |= ((value as u32) & mask) << (lane * width);
            if mode == 0 {
                let flag = if signed || subtract {
                    raw >= 0
                } else {
                    raw > i64::from(mask)
                };
                if flag {
                    ge |= if width == 8 {
                        1 << lane
                    } else {
                        3 << (lane * 2)
                    };
                }
            }
        }
        if mode == 0 {
            self.psr.value = (self.psr.value & !0x000f0000) | (ge << 16);
        }
        self.set_r(rd, result);
        Ok(ExecuteSuccess::Taken { cycles: 1 })
    }
}

#[cfg(all(test, feature = "has-dsp-ext"))]
mod tests {
    use super::*;
    #[test]
    fn invalid_registers_fault_before_changing_registers_or_flags() {
        for operation in [8, 9, 10, 12, 13, 14] {
            for flavor in [0, 1, 2, 4, 5, 6] {
                let base = 0xfa00f002 | (operation << 20) | (1 << 16) | (flavor << 4);
                for bad in [13u32, 15] {
                    for shift in [0, 8, 16] {
                        let mut cpu = Processor::new();
                        cpu.r0_12.fill(0xdeadbeef);
                        cpu.psr.value = 0xa90a0000;
                        let opcode = (base & !(15 << shift)) | (bad << shift);
                        assert_eq!(cpu.exec_parallel_dsp(opcode), Err(Fault::UndefInstr));
                        assert_eq!(cpu.r0_12, [0xdeadbeef; 13]);
                        assert_eq!(cpu.psr.value, 0xa90a0000);
                    }
                }
            }
        }
    }
}
