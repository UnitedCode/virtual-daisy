use super::ExecuteResult;
// Modified for Virtual Daisy's browser backend; see UPSTREAM.md.
use crate::Processor;
use crate::executor::{ExecuteSuccess, ExecutorHelper};

use crate::{
    bus::Bus,
    core::{
        instruction::LoadAndStoreMultipleParams,
        register::{BaseReg, Reg},
    },
};
use enum_set::EnumSet;

/// Load and Store operations
pub trait IsaLoadAndStoreMultiple {
    fn exec_stm(&mut self, params: &LoadAndStoreMultipleParams) -> ExecuteResult;
    fn exec_stmdb(&mut self, params: &LoadAndStoreMultipleParams) -> ExecuteResult;
    fn exec_ldm(&mut self, params: &LoadAndStoreMultipleParams) -> ExecuteResult;
    fn exec_push(&mut self, registers: EnumSet<Reg>) -> ExecuteResult;
    fn exec_pop(&mut self, registers: EnumSet<Reg>) -> ExecuteResult;
}

impl IsaLoadAndStoreMultiple for Processor {
    fn exec_stm(&mut self, params: &LoadAndStoreMultipleParams) -> ExecuteResult {
        if self.condition_passed() {
            let regs_size = 4 * (params.registers.len() as u32);

            let mut address = self.get_r(params.rn);

            for reg in &params.registers {
                let r = self.get_r(reg);
                self.write32(address, r)?;
                address += 4;
            }

            if params.wback {
                self.add_r(params.rn, regs_size);
            }
            return Ok(ExecuteSuccess::Taken {
                cycles: 1 + params.registers.len() as u32,
            });
        }
        Ok(ExecuteSuccess::NotTaken)
    }

    fn exec_stmdb(&mut self, params: &LoadAndStoreMultipleParams) -> ExecuteResult {
        if self.condition_passed() {
            let regs_size = 4 * (params.registers.len() as u32);

            let mut address = self.get_r(params.rn) - regs_size;

            for reg in &params.registers {
                let r = self.get_r(reg);
                self.write32(address, r)?;
                address += 4;
            }

            if params.wback {
                self.sub_r(params.rn, regs_size);
            }
            return Ok(ExecuteSuccess::Taken {
                cycles: 1 + params.registers.len() as u32,
            });
        }
        Ok(ExecuteSuccess::NotTaken)
    }

    fn exec_ldm(&mut self, params: &LoadAndStoreMultipleParams) -> ExecuteResult {
        if self.condition_passed() {
            let regs_size = 4 * (params.registers.len() as u32);

            let mut address = self.get_r(params.rn);

            let mut loaded_pc = None;
            for reg in &params.registers {
                let value = self.read32(address)?;
                if reg == Reg::PC {
                    loaded_pc = Some(value);
                } else {
                    self.set_r(reg, value);
                }
                address += 4;
            }

            if params.wback && !params.registers.contains(&params.rn) {
                self.add_r(params.rn, regs_size);
            }
            let cc = 1 + params.registers.len() as u32;
            if let Some(value) = loaded_pc {
                // Exception return consumes the hardware frame at the updated
                // SP and restores GPRs. Complete writeback before unstacking.
                self.load_write_pc(value)?;
                return Ok(ExecuteSuccess::Branched { cycles: cc });
            }
            return Ok(ExecuteSuccess::Taken { cycles: cc });
        }
        Ok(ExecuteSuccess::NotTaken)
    }

    fn exec_push(&mut self, registers: EnumSet<Reg>) -> ExecuteResult {
        if self.condition_passed() {
            let regs_size = 4 * (registers.len() as u32);
            let sp = self.get_r(Reg::SP);
            let mut address = sp - regs_size;

            for reg in &registers {
                let value = self.get_r(reg);
                self.write32(address, value)?;
                address += 4;
            }

            self.set_r(Reg::SP, sp - regs_size);
            return Ok(ExecuteSuccess::Taken {
                cycles: 1 + registers.len() as u32,
            });
        }
        Ok(ExecuteSuccess::NotTaken)
    }

    fn exec_pop(&mut self, registers: EnumSet<Reg>) -> ExecuteResult {
        if self.condition_passed() {
            let regs_size = 4 * (registers.len() as u32);
            let sp = self.get_r(Reg::SP);
            let mut address = sp;

            self.set_r(Reg::SP, sp + regs_size);

            for reg in &registers {
                let val = self.read32(address)?;
                if reg == Reg::PC {
                    self.bx_write_pc(val)?;
                } else {
                    self.set_r(reg, val);
                }
                address += 4;
            }

            if registers.contains(&Reg::PC) {
                return Ok(ExecuteSuccess::Branched {
                    cycles: 4 + registers.len() as u32,
                });
            } else {
                return Ok(ExecuteSuccess::Taken {
                    cycles: 1 + registers.len() as u32,
                });
            }
        }
        Ok(ExecuteSuccess::NotTaken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;
    use crate::core::{instruction::LoadAndStoreMultipleParams, operation::get_reglist};

    #[cfg(feature = "has-fp")]
    #[test]
    fn ldm_sp_writeback_precedes_basic_and_extended_exception_unstacking() {
        use crate::core::{
            exception::{Exception, ExceptionHandling},
            register::BaseReg,
        };
        for fp in [false, true] {
            let mut cpu = Processor::new();
            let mut image = vec![0; 256];
            image[15 * 4..16 * 4].copy_from_slice(&0x41u32.to_le_bytes());
            cpu.flash_memory(image.len(), &image);
            cpu.psr.value = 1 << 24;
            cpu.set_r(Reg::SP, 0x20001000);
            cpu.set_r(Reg::R0, 3);
            cpu.set_r(Reg::R12, 7);
            cpu.cpacr = 0x00f00000;
            cpu.fpccr = 0;
            cpu.control.fpca = fp;
            cpu.fp_regs[0] = 1f32.to_bits();
            cpu.exception_entry(Exception::SysTick, 0x20).unwrap();
            let return_code = cpu.get_r(Reg::LR);
            let sp = cpu.get_r(Reg::SP) - 8;
            cpu.set_r(Reg::SP, sp);
            cpu.write32(sp, 0xaaaaaaaa).unwrap();
            cpu.write32(sp + 4, return_code).unwrap();
            cpu.exec_ldm(&LoadAndStoreMultipleParams {
                registers: get_reglist((1 << 4) | (1 << 15)),
                rn: Reg::SP,
                wback: true,
            })
            .unwrap();
            assert_eq!(cpu.get_r(Reg::SP), 0x20001000);
            assert_eq!(cpu.get_r(Reg::R0), 3);
            assert_eq!(cpu.get_r(Reg::R12), 7);
            assert_eq!(cpu.get_r(Reg::R4), 0xaaaaaaaa);
            assert_eq!(cpu.get_pc(), 0x20);
        }
    }

    #[test]
    fn test_exec_ldm_with_writeback_updates_base_register() {
        let mut core = Processor::new();
        let base = 0x2000_0000;

        core.write32(base, 0x1111_1111).unwrap();
        core.write32(base + 4, 0x2222_2222).unwrap();
        core.set_r(Reg::R2, base);

        core.exec_ldm(&LoadAndStoreMultipleParams {
            registers: get_reglist(0b0000_0011),
            rn: Reg::R2,
            wback: true,
        })
        .unwrap();

        assert_eq!(core.get_r(Reg::R0), 0x1111_1111);
        assert_eq!(core.get_r(Reg::R1), 0x2222_2222);
        assert_eq!(core.get_r(Reg::R2), base + 8);
    }

    #[test]
    fn test_exec_ldm_without_writeback_preserves_base_register() {
        let mut core = Processor::new();
        let base = 0x2000_0000;

        core.write32(base, 0x3333_3333).unwrap();
        core.write32(base + 4, 0xCCCC_CCCC).unwrap();
        core.set_r(Reg::R1, base);

        core.exec_ldm(&LoadAndStoreMultipleParams {
            registers: get_reglist(0x1008),
            rn: Reg::R1,
            wback: false,
        })
        .unwrap();

        assert_eq!(core.get_r(Reg::R3), 0x3333_3333);
        assert_eq!(core.get_r(Reg::R12), 0xCCCC_CCCC);
        assert_eq!(core.get_r(Reg::R1), base);
    }
}
