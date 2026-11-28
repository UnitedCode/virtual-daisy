use super::*;
use crate::bus::Bus;
use crate::core::fault::FaultTrapMode;

fn fixture() -> Processor {
    let mut bytes = [0u8; 512];
    for (at, value) in [
        (0, 0x2001fff0u32),
        (4, 0x101),
        (12, 0x181),
        (16, 0x1a1),
        (20, 0x1c1),
        (24, 0x1e1),
        (44, 0x101),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut cpu = Processor::new();
    cpu.flash_memory(bytes.len(), &bytes);
    cpu.msp = 0x2001fff0;
    cpu.psr.value = 1 << 24;
    cpu.running = true;
    cpu.fault_trap_mode(FaultTrapMode::none());
    cpu.write32(0xe000ed24, 7 << 16).unwrap();
    cpu
}

#[test]
fn svc_stacks_the_following_pc_and_it_slot_even_when_it_escalates() {
    use crate::core::thumb::ThumbCode;

    for (primask, basepri, active_priority, group, escalated) in [
        (false, 0, None, 0, false),
        (true, 0, None, 0, true),
        (false, 0x40, None, 0, true),
        (false, 0x80, None, 0, false),
        (false, 0, Some(0x80), 0, false),
        (false, 0, Some(0x50), 4, true),
    ] {
        let mut cpu = fixture();
        cpu.primask = primask;
        cpu.basepri = basepri;
        cpu.aircr = group << 8;
        cpu.set_exception_priority(Exception::SVCall, 0x40);
        if let Some(priority) = active_priority {
            cpu.set_exception_priority(Exception::SysTick, priority);
            cpu.test_set_active_exception(Exception::SysTick);
        }
        cpu.psr.set_z_bit(true);
        cpu.branch_write_pc(0x120);
        let itt = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf04 });
        cpu.execute(&itt, 2);
        let svc = cpu.decode(ThumbCode::Thumb16 { opcode: 0xdf00 });
        cpu.execute(&svc, 2);
        assert_eq!(cpu.psr.get_isr_number(), if escalated { 3 } else { 11 });
        assert_eq!(cpu.hfsr, if escalated { 1 << 30 } else { 0 });
        assert_eq!(cpu.cfsr, 0);
        assert_eq!(cpu.read32(cpu.msp + 24).unwrap(), 0x124);
        assert_eq!(cpu.read32(cpu.msp + 28).unwrap() & 0x0600_fc00, 0x800);
        assert_eq!(cpu.get_itstate(), 0);
    }
}

#[test]
fn svc_at_hardfault_priority_locks_up_at_the_instruction_address() {
    let mut cpu = fixture();
    cpu.faultmask = true;
    let sp = cpu.msp;
    cpu.branch_write_pc(0x120);
    cpu.execute(&Instruction::SVC { imm32: 0 }, 2);
    assert!(!cpu.running);
    assert_eq!(cpu.msp, sp);
    let trap = cpu.take_pending_fault_trap().unwrap();
    assert_eq!(trap.trap_reason, FaultTrapReason::Lockup);
    assert_eq!(trap.fault, Fault::Forced);
    assert_eq!(trap.pc, 0x120);
}

#[test]
fn synchronous_faults_escalate_when_masks_or_group_priority_prevent_preemption() {
    // fault priority, active SysTick priority, PRIGROUP, BASEPRI, PRIMASK,
    // expected escalation. Smaller priority numbers mean higher priority.
    let cases = [
        (0x80, None, 0, 0, false, false),
        (0x80, None, 0, 0, true, true),
        (0x80, None, 0, 0x80, false, true),
        (0x40, None, 0, 0x80, false, false),
        (0x80, None, 0, 0x40, false, true),
        (0x40, Some(0x80), 0, 0, false, false),
        (0x80, Some(0x80), 0, 0, false, true),
        (0xc0, Some(0x80), 0, 0, false, true),
        (0x40, Some(0x50), 4, 0, false, true),
        (0x40, Some(0x80), 4, 0, false, false),
        (0x40, Some(0x80), 7, 0, false, true),
        (0, Some(0x80), 7, 0, false, true),
        (0x40, None, 4, 0x50, false, true),
    ];
    for (fault, exception, status) in [
        (Fault::DAccViol, Exception::MemoryManagementFault, 1 << 1),
        (Fault::Preciserr, Exception::BusFault, 1 << 9),
        (Fault::UndefInstr, Exception::UsageFault, 1 << 16),
    ] {
        for (priority, active, group, basepri, primask, escalated) in cases {
            let mut cpu = fixture();
            cpu.set_exception_priority(exception, priority);
            if let Some(priority) = active {
                cpu.set_exception_priority(Exception::SysTick, priority);
                cpu.test_set_active_exception(Exception::SysTick);
            }
            cpu.aircr = group << 8;
            cpu.basepri = basepri;
            cpu.primask = primask;
            cpu.handle_fault(fault, 0x100);
            let expected = if escalated {
                Exception::HardFault
            } else {
                exception
            };
            assert_eq!(
                cpu.psr.get_isr_number(),
                usize::from(expected),
                "{fault:?}, priority={priority:x}, active={active:?}, group={group}, basepri={basepri:x}, primask={primask}"
            );
            assert_eq!(cpu.cfsr, status);
            assert_eq!(cpu.hfsr, if escalated { 1 << 30 } else { 0 });
            assert_eq!(cpu.read32(cpu.msp + 24).unwrap(), 0x100);
        }
    }
}

#[test]
fn hardfault_priority_locks_up_without_another_stack_frame() {
    for active in [None, Some(Exception::HardFault), Some(Exception::NMI)] {
        for fault in [
            Fault::DAccViol,
            Fault::Preciserr,
            Fault::UndefInstr,
            Fault::VectorTable,
        ] {
            let mut cpu = fixture();
            cpu.faultmask = active.is_none();
            if let Some(exception) = active {
                cpu.test_set_active_exception(exception);
            }
            let sp = cpu.msp;
            cpu.handle_fault(fault, 0x100);
            assert!(!cpu.running, "{fault:?}");
            assert_eq!(cpu.msp, sp);
            assert_eq!(cpu.psr.get_isr_number(), active.map_or(0, usize::from));
            let trap = cpu
                .take_pending_fault_trap()
                .expect("lockup is always reported");
            assert_eq!(trap.trap_reason, FaultTrapReason::Lockup);
            assert_eq!(trap.exception, Exception::HardFault);
            assert_eq!(trap.fault, fault);
            assert_eq!(trap.pc, 0x100);
            assert_eq!(trap.active_exception, active);
            assert_eq!(
                cpu.hfsr,
                if fault == Fault::VectorTable {
                    2
                } else {
                    1 << 30
                }
            );
        }
    }
}
