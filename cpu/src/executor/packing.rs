use crate::Processor;

use crate::executor::{ExecuteSuccess, ExecutorHelper};

use super::ExecuteResult;
use crate::core::instruction::{Reg2UsizeParams, Reg3UsizeParams};
use crate::core::{
    bits::Bits,
    fault::Fault,
    operation::{ror, sign_extend},
    register::{BaseReg, Reg},
};

/// Sign and zero extension operations.
pub trait IsaPacking {
    fn exec_sxtb(&mut self, params: &Reg2UsizeParams) -> ExecuteResult;
    fn exec_sxth(&mut self, params: &Reg2UsizeParams) -> ExecuteResult;
    fn exec_uxtb(&mut self, params: &Reg2UsizeParams) -> ExecuteResult;
    fn exec_uxth(&mut self, params: &Reg2UsizeParams) -> ExecuteResult;
    fn exec_uxtab(&mut self, params: &Reg3UsizeParams) -> ExecuteResult;
}

impl IsaPacking for Processor {
    fn exec_sxtb(&mut self, params: &Reg2UsizeParams) -> ExecuteResult {
        if self.condition_passed() {
            validate(params)?;
            let rm = self.get_r(params.rm);
            let rotated = ror(rm, params.rotation);
            let result = sign_extend(rotated.get_bits(0..8), 7, 32) as u32;
            self.set_r(params.rd, result);
            return Ok(ExecuteSuccess::Taken { cycles: 1 });
        }
        Ok(ExecuteSuccess::NotTaken)
    }

    fn exec_sxth(&mut self, params: &Reg2UsizeParams) -> ExecuteResult {
        if self.condition_passed() {
            validate(params)?;
            let rm = self.get_r(params.rm);
            let rotated = ror(rm, params.rotation);
            let result = sign_extend(rotated.get_bits(0..16), 15, 32) as u32;
            self.set_r(params.rd, result);
            return Ok(ExecuteSuccess::Taken { cycles: 1 });
        }
        Ok(ExecuteSuccess::NotTaken)
    }

    fn exec_uxtb(&mut self, params: &Reg2UsizeParams) -> ExecuteResult {
        if self.condition_passed() {
            validate(params)?;
            let rm = self.get_r(params.rm);
            let rotated = ror(rm, params.rotation);
            self.set_r(params.rd, rotated.get_bits(0..8));
            return Ok(ExecuteSuccess::Taken { cycles: 1 });
        }
        Ok(ExecuteSuccess::NotTaken)
    }

    fn exec_uxth(&mut self, params: &Reg2UsizeParams) -> ExecuteResult {
        if self.condition_passed() {
            validate(params)?;
            let rm = self.get_r(params.rm);
            let rotated = ror(rm, params.rotation);
            self.set_r(params.rd, rotated.get_bits(0..16));
            return Ok(ExecuteSuccess::Taken { cycles: 1 });
        }
        Ok(ExecuteSuccess::NotTaken)
    }

    fn exec_uxtab(&mut self, params: &Reg3UsizeParams) -> ExecuteResult {
        if self.condition_passed() {
            if !cfg!(feature = "has-dsp-ext")
                || [params.rd, params.rn, params.rm]
                    .iter()
                    .any(|r| matches!(r, Reg::SP | Reg::PC))
                || !matches!(params.rotation, 0 | 8 | 16 | 24)
            {
                return Err(Fault::UndefInstr);
            }
            let rm = self.get_r(params.rm);
            let rotated = ror(rm, params.rotation);
            let rn = self.get_r(params.rn);
            let result = rn.wrapping_add(rotated.get_bits(0..8));
            self.set_r(params.rd, result);
            return Ok(ExecuteSuccess::Taken { cycles: 1 });
        }
        Ok(ExecuteSuccess::NotTaken)
    }
}

fn validate(params: &Reg2UsizeParams) -> Result<(), Fault> {
    if [params.rd, params.rm]
        .iter()
        .any(|r| matches!(r, Reg::SP | Reg::PC))
        || !matches!(params.rotation, 0 | 8 | 16 | 24)
    {
        Err(Fault::UndefInstr)
    } else {
        Ok(())
    }
}

impl Processor {
    pub(super) fn exec_dsp_extend(&mut self, opcode: u32) -> ExecuteResult {
        if !self.condition_passed() {
            return Ok(ExecuteSuccess::NotTaken);
        }
        let group = (opcode >> 20) & 7;
        let rd = Reg::from(((opcode >> 8) & 15) as u8);
        let rn = Reg::from(((opcode >> 16) & 15) as u8);
        let rm = Reg::from((opcode & 15) as u8);
        if !cfg!(feature = "has-dsp-ext")
            || opcode & 0xff80f0c0 != 0xfa00f080
            || group > 5
            || rn == Reg::SP
            || [rd, rm].iter().any(|r| matches!(r, Reg::SP | Reg::PC))
        {
            return Err(Fault::UndefInstr);
        }
        // Rn=PC selects the non-accumulating alias; it does not read PC.
        let a = if rn == Reg::PC { 0 } else { self.get_r(rn) };
        let b = self.get_r(rm).rotate_right(((opcode >> 4) & 3) * 8);
        let signed = group & 1 == 0;
        let byte = |v: u32| {
            if signed {
                i32::from(v as i8) as u32
            } else {
                v & 255
            }
        };
        let result = match group {
            0 => a.wrapping_add(i32::from(b as i16) as u32),
            1 => a.wrapping_add(b & 0xffff),
            2 | 3 => {
                let lo = (a & 0xffff).wrapping_add(byte(b)) & 0xffff;
                let hi = (a >> 16).wrapping_add(byte(b >> 16)) & 0xffff;
                lo | hi << 16
            }
            4 | 5 => a.wrapping_add(byte(b)),
            _ => unreachable!(),
        };
        self.set_r(rd, result);
        Ok(ExecuteSuccess::Taken { cycles: 1 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_reject_invalid_operands_without_writes() {
        for bad in [Reg::SP, Reg::PC] {
            for field in 0..2 {
                let mut cpu = Processor::new();
                cpu.r0_12.fill(0xdeadbeef);
                let params = Reg2UsizeParams {
                    rd: if field == 0 { bad } else { Reg::R0 },
                    rm: if field == 1 { bad } else { Reg::R1 },
                    rotation: 0,
                };
                assert_eq!(cpu.exec_sxtb(&params), Err(Fault::UndefInstr));
                assert_eq!(cpu.exec_sxth(&params), Err(Fault::UndefInstr));
                assert_eq!(cpu.exec_uxtb(&params), Err(Fault::UndefInstr));
                assert_eq!(cpu.exec_uxth(&params), Err(Fault::UndefInstr));
                assert_eq!(cpu.r0_12, [0xdeadbeef; 13]);
            }
        }
        for rotation in [1, 7, 32, usize::MAX] {
            let mut cpu = Processor::new();
            assert_eq!(
                cpu.exec_sxtb(&Reg2UsizeParams {
                    rd: Reg::R0,
                    rm: Reg::R1,
                    rotation
                }),
                Err(Fault::UndefInstr)
            );
        }
    }

    #[test]
    fn dsp_extensions_and_packing_validate_direct_execution() {
        for base in [
            0xfa01f082, 0xfa11f082, 0xfa21f082, 0xfa31f082, 0xfa41f082, 0xfa51f082, 0xeac10002,
            0xeac10022,
        ] {
            for bad in [13u32, 15] {
                for shift in [0, 8, 16] {
                    if base >> 24 == 0xfa && bad == 15 && shift == 16 {
                        continue;
                    }
                    let opcode = (base & !(15 << shift)) | bad << shift;
                    let mut cpu = Processor::new();
                    cpu.r0_12.fill(0xdeadbeef);
                    assert_eq!(
                        cpu.exec_browser_dsp(opcode),
                        Err(Fault::UndefInstr),
                        "{opcode:08x}"
                    );
                    assert_eq!(cpu.r0_12, [0xdeadbeef; 13]);
                }
            }
            if !cfg!(feature = "has-dsp-ext") {
                assert_eq!(
                    Processor::new().exec_browser_dsp(base),
                    Err(Fault::UndefInstr)
                );
            }
        }
        if !cfg!(feature = "has-dsp-ext") {
            assert_eq!(
                Processor::new().execute_internal(&crate::core::instruction::Instruction::UXTB16 {
                    params: Reg2UsizeParams {
                        rd: Reg::R0,
                        rm: Reg::R1,
                        rotation: 0
                    }
                }),
                Err(Fault::UndefInstr)
            );
            assert_eq!(
                Processor::new().exec_uxtab(&Reg3UsizeParams {
                    rd: Reg::R0,
                    rn: Reg::R1,
                    rm: Reg::R2,
                    rotation: 0
                }),
                Err(Fault::UndefInstr)
            );
        }
    }
}
