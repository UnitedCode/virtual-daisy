// ARM DUI 0646B: SSAT shifts a 32-bit operand, saturates its signed value,
// sets sticky Q on saturation, and leaves N/Z/C/V unchanged.
use super::{ExecuteResult, ExecuteSuccess, ExecutorHelper};
use crate::{
    Processor,
    core::{
        instruction::SRType,
        operation::shift,
        register::{Apsr, BaseReg, Reg},
    },
};
pub trait IsaSaturate {
    fn exec_usat(
        &mut self,
        rd: Reg,
        rn: Reg,
        bits: u32,
        shift_t: SRType,
        amount: usize,
    ) -> ExecuteResult;
    #[cfg(feature = "has-dsp-ext")]
    fn exec_sat16(&mut self, rd: Reg, rn: Reg, bits: u32, unsigned: bool) -> ExecuteResult;
    fn exec_ssat(
        &mut self,
        rd: Reg,
        rn: Reg,
        bits: u32,
        shift_t: SRType,
        amount: usize,
    ) -> ExecuteResult;
}
fn saturate(value: i64, bits: u32, unsigned: bool) -> (u32, bool) {
    let (low, high) = if unsigned {
        (0, (1i64 << bits) - 1)
    } else {
        let limit = 1i64 << (bits - 1);
        (-limit, limit - 1)
    };
    let result = value.clamp(low, high);
    (result as u32, result != value)
}
impl IsaSaturate for Processor {
    fn exec_usat(
        &mut self,
        rd: Reg,
        rn: Reg,
        bits: u32,
        shift_t: SRType,
        amount: usize,
    ) -> ExecuteResult {
        if !self.condition_passed() {
            return Ok(ExecuteSuccess::NotTaken);
        }
        let value = i64::from(shift(self.get_r(rn), shift_t, amount, self.psr.get_c()) as i32);
        let (result, clipped) = saturate(value, bits, true);
        if clipped {
            self.psr.set_q(true);
        }
        self.set_r(rd, result);
        Ok(ExecuteSuccess::Taken { cycles: 1 })
    }
    #[cfg(feature = "has-dsp-ext")]
    fn exec_sat16(&mut self, rd: Reg, rn: Reg, bits: u32, unsigned: bool) -> ExecuteResult {
        if !self.condition_passed() {
            return Ok(ExecuteSuccess::NotTaken);
        }
        let source = self.get_r(rn);
        let mut result = 0;
        for offset in [0, 16] {
            let value = i64::from((source >> offset) as i16);
            let (lane, clipped) = saturate(value, bits, unsigned);
            result |= (lane & 0xffff) << offset;
            if clipped {
                self.psr.set_q(true);
            }
        }
        self.set_r(rd, result);
        Ok(ExecuteSuccess::Taken { cycles: 1 })
    }
    fn exec_ssat(
        &mut self,
        rd: Reg,
        rn: Reg,
        bits: u32,
        shift_t: SRType,
        amount: usize,
    ) -> ExecuteResult {
        if !self.condition_passed() {
            return Ok(ExecuteSuccess::NotTaken);
        }
        let value = i64::from(shift(self.get_r(rn), shift_t, amount, self.psr.get_c()) as i32);
        let (result, clipped) = saturate(value, bits, false);
        if clipped {
            self.psr.set_q(true);
        }
        self.set_r(rd, result);
        Ok(ExecuteSuccess::Taken { cycles: 1 })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{core::instruction::Instruction, decoder::Decoder};
    fn decode(opcode: u32) -> Instruction {
        Processor::new().decode(opcode.into())
    }
    #[test]
    fn ssat_decodes_and_executes_the_compiled_overdub_opcode_with_sticky_q() {
        let ins = decode(0xf301_0117);
        assert_eq!(
            ins,
            Instruction::SSAT {
                rd: Reg::R1,
                rn: Reg::R1,
                saturate_to: 24,
                shift_t: SRType::LSL,
                shift_n: 0
            }
        );
        let mut p = Processor::new();
        p.psr.set_n_bit(true);
        p.psr.set_c(true);
        p.psr.set_v(true);
        let nzcv = (p.psr.get_n(), p.psr.get_z(), p.psr.get_c(), p.psr.get_v());
        for (value, result, q) in [
            (8_388_608i32, 8_388_607i32, true),
            (-8_388_609, -8_388_608, true),
            (5, 5, false),
            (i32::MIN, -8_388_608, true),
        ] {
            p.psr.set_q(false);
            p.set_r(Reg::R1, value as u32);
            p.execute_internal(&ins).unwrap();
            assert_eq!(p.get_r(Reg::R1), result as u32);
            assert_eq!(p.psr.get_q(), q);
            assert_eq!(
                (p.psr.get_n(), p.psr.get_z(), p.psr.get_c(), p.psr.get_v()),
                nzcv
            );
        }
        p.set_r(Reg::R1, 0);
        p.execute_internal(&ins).unwrap();
        assert!(p.psr.get_q());
    }
    #[test]
    fn ssat_shift_and_signed_boundary_cases() {
        let mut p = Processor::new();
        for (value, bits, kind, n, expected) in [
            (0x4000_0000, 32, SRType::LSL, 1, i32::MIN),
            (-17i32 as u32, 4, SRType::ASR, 1, -8),
            (8, 4, SRType::LSL, 0, 7),
            (1, 1, SRType::LSL, 0, 0),
            (i32::MIN as u32, 32, SRType::LSL, 0, i32::MIN),
        ] {
            p.set_r(Reg::R0, value);
            p.exec_ssat(Reg::R1, Reg::R0, bits, kind, n).unwrap();
            assert_eq!(p.get_r(Reg::R1), expected as u32);
        }
        #[cfg(not(feature = "has-dsp-ext"))]
        assert!(matches!(decode(0xf321_0107), Instruction::UDF { .. }));
        #[cfg(feature = "has-dsp-ext")]
        assert!(matches!(
            decode(0xf321_0107),
            Instruction::SSAT16 { saturate_to: 8, .. }
        ));
        assert!(matches!(decode(0xf301_0f17), Instruction::UDF { .. }));
        assert!(matches!(
            decode(0xf301_2117),
            Instruction::SSAT { shift_n: 8, .. }
        ));
        assert!(matches!(
            decode(0xf321_0157),
            Instruction::SSAT {
                shift_t: SRType::ASR,
                shift_n: 1,
                ..
            }
        ));
    }
}
