#![cfg(not(feature = "armv6m"))]
use zmu_cortex_m::{
    Processor,
    bus::Bus,
    core::{
        exception::{Exception, ExceptionHandling},
        instruction::{Instruction, MsrParams},
        register::{BaseReg, Ipsr, Reg},
        reset::Reset,
    },
    executor::Executor,
    semihosting::{SemihostingCommand, SemihostingResponse},
};

fn fixture() -> Processor {
    let mut bytes = vec![0; 256];
    for (offset, word) in [(0, 0x20010000u32), (4, 0x81), (12, 0xc1), (48, 0xe1)] {
        bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    let mut c = Processor::new();
    c.flash_memory(bytes.len(), &bytes);
    c.reset().unwrap();
    c
}

#[test]
fn unhandled_breakpoints_fault_without_decoding_semihost_arguments() {
    for imm32 in [0, 0xab, 255] {
        let mut c = fixture();
        c.set_r(Reg::R0, u32::MAX); // Unknown semihosting operation.
        c.set_r(Reg::R1, u32::MAX); // Must not dereference without a host handler.
        c.execute(&Instruction::BKPT { imm32 }, 2);
        assert_eq!(c.psr.get_isr_number(), 3);
        assert_eq!(c.get_pc(), 0xc0);
        assert_eq!(c.read32(c.get_msp() + 24), Ok(0x80));
        assert_eq!(c.hfsr, 1 << 31);
        assert_eq!(c.dfsr, 1 << 1);
        assert_eq!(c.cfsr, 0);
        c.write32(0xe000ed30, 0).unwrap();
        assert_eq!(c.dfsr, 2);
        c.write32(0xe000ed30, 2).unwrap();
        assert_eq!(c.dfsr, 0);
    }
}

#[test]
fn enabled_monitor_uses_vector_twelve_and_respects_priority_masks() {
    assert_eq!(usize::from(Exception::DebugMonitor), 12);
    assert_eq!(Exception::from(12), Exception::DebugMonitor);
    for blocked in [false, true] {
        let mut c = fixture();
        c.write32(0xe000edfc, 1 << 16).unwrap();
        c.set_exception_priority(Exception::DebugMonitor, 32);
        c.set_r(Reg::R0, u32::from(blocked));
        c.execute(
            &Instruction::MSR_reg {
                params: MsrParams {
                    rn: Reg::R0,
                    sysm: 16,
                    mask: 0,
                },
            },
            4,
        );
        c.set_pc(0x80);
        c.execute(&Instruction::BKPT { imm32: 7 }, 2);
        assert_eq!(c.psr.get_isr_number(), if blocked { 3 } else { 12 });
        assert_eq!(c.get_pc(), if blocked { 0xc0 } else { 0xe0 });
        assert_eq!(c.read32(c.get_msp() + 24), Ok(0x80));
        assert_eq!(c.hfsr, if blocked { 1 << 31 } else { 0 });
        assert_eq!(c.dfsr, 2);
    }
}

#[test]
fn monitor_group_priority_cannot_preempt_equal_group() {
    let mut c = fixture();
    c.write32(0xe000edfc, 1 << 16).unwrap();
    c.aircr = 4 << 8; // Five low subpriority bits.
    c.set_exception_priority(Exception::DebugMonitor, 32);
    c.set_r(Reg::R0, 63); // Same group despite the numerically higher raw priority.
    c.execute(
        &Instruction::MSR_reg {
            params: MsrParams {
                rn: Reg::R0,
                sysm: 17,
                mask: 0,
            },
        },
        4,
    );
    c.set_pc(0x80);
    c.execute(&Instruction::BKPT { imm32: 0 }, 2);
    assert_eq!(c.psr.get_isr_number(), 3);
    assert_eq!(c.hfsr, 1 << 31);
}

#[test]
fn breakpoint_in_hardfault_locks_up_instead_of_recursing() {
    let mut c = fixture();
    c.execute(&Instruction::BKPT { imm32: 0 }, 2);
    c.execute(&Instruction::BKPT { imm32: 0 }, 2);
    let trap = c.take_pending_fault_trap().unwrap();
    assert_eq!(
        trap.trap_reason,
        zmu_cortex_m::core::fault::FaultTrapReason::Lockup
    );
    assert_eq!(c.get_msp(), 0x2000ffe0);
}

#[test]
fn software_monitor_pending_is_visible_and_can_be_cleared() {
    let mut c = fixture();
    c.write32(0xe000edfc, 1 << 17).unwrap();
    assert!(c.exception_pending(Exception::DebugMonitor));
    assert_eq!(c.read32(0xe000edfc), Ok(1 << 17));
    c.write32(0xe000edfc, 0).unwrap();
    assert!(!c.exception_pending(Exception::DebugMonitor));
    c.write32(0xe000edfc, 1 << 17).unwrap();
    c.check_exceptions();
    assert_eq!(c.psr.get_isr_number(), 12);
    assert_eq!(c.read32(0xe000edfc), Ok(0));
}

#[test]
fn explicit_semihosting_handler_still_intercepts_supported_commands() {
    let mut c = fixture();
    c.semihost(Some(Box::new(|command| {
        assert_eq!(*command, SemihostingCommand::SysClock);
        SemihostingResponse::SysClock { result: Ok(123) }
    })));
    c.set_r(Reg::R0, 0x10);
    c.execute(&Instruction::BKPT { imm32: 0xab }, 2);
    assert_eq!(c.get_pc(), 0x82);
    assert_eq!(c.get_r(Reg::R0), 123);
    assert_eq!(c.hfsr, 0);
    c.set_r(Reg::R0, u32::MAX);
    c.execute(&Instruction::BKPT { imm32: 0xab }, 2);
    assert_eq!(c.psr.get_isr_number(), 3);
    assert_eq!(c.hfsr, 1 << 31);
}

#[test]
fn invalid_semihost_filename_does_not_panic_the_host() {
    let mut c = fixture();
    c.semihost(Some(Box::new(|_| {
        panic!("invalid command reached handler")
    })));
    c.write32(0x20000000, 0x20000010).unwrap();
    c.write32(0x20000004, 0).unwrap();
    c.write32(0x20000008, 1).unwrap();
    c.write8(0x20000010, 0xff).unwrap();
    c.set_r(Reg::R0, 1); // SYS_OPEN
    c.set_r(Reg::R1, 0x20000000);
    c.execute(&Instruction::BKPT { imm32: 0xab }, 2);
    assert_eq!(c.psr.get_isr_number(), 3);
    assert_eq!(c.hfsr, 1 << 31);
}
