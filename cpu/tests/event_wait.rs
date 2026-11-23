#![cfg(any(feature = "armv7m", feature = "armv7em"))]
use zmu_cortex_m::{
    Processor,
    bus::Bus,
    core::{reset::Reset, thumb::ThumbCode},
    decoder::Decoder,
    executor::Executor,
};
fn cpu() -> Processor {
    let mut cpu = Processor::new();
    cpu.reset().unwrap();
    cpu.psr.value |= 1 << 24;
    cpu
}
fn execute(cpu: &mut Processor, opcode: u16, compiled: bool) {
    let instruction = cpu.decode(ThumbCode::Thumb16 { opcode });
    if compiled {
        cpu.execute_compiled(&instruction, 2);
    } else {
        cpu.execute(&instruction, 2);
    }
}
#[test]
fn sev_latches_one_event_and_wfe_consumes_it_before_sleeping() {
    for compiled in [false, true] {
        let mut c = cpu();
        execute(&mut c, 0xbf40, compiled);
        execute(&mut c, 0xbf40, compiled);
        execute(&mut c, 0xbf20, compiled);
        assert!(!c.sleeping);
        execute(&mut c, 0xbf20, compiled);
        assert!(
            c.sleeping,
            "WFE must wait after the single event was consumed"
        );
    }
}
#[test]
fn skipped_wfe_does_not_consume_the_latched_event() {
    for compiled in [false, true] {
        let mut c = cpu();
        c.psr.value = 0x51050000;
        let before = c.psr.value & 0xf80f0000;
        execute(&mut c, 0xbf40, compiled);
        execute(&mut c, 0xbf18, compiled);
        execute(&mut c, 0xbf20, compiled);
        assert!(!c.sleeping);
        execute(&mut c, 0xbf20, compiled);
        assert!(!c.sleeping);
        execute(&mut c, 0xbf20, compiled);
        assert!(c.sleeping);
        assert_eq!(c.psr.value & 0xf80f0000, before);
    }
}
#[test]
fn disabled_interrupt_pending_edges_obey_sevonpend() {
    for compiled in [false, true] {
        let mut c = cpu();
        c.write32(0xe000ed10, 1 << 4).unwrap();
        c.write32(0xe000e200, 1 << 5).unwrap();
        execute(&mut c, 0xbf20, compiled);
        assert!(!c.sleeping);
        execute(&mut c, 0xbf20, compiled);
        assert!(c.sleeping);
        c.write32(0xe000e200, 1 << 5).unwrap();
        c.step_sleep();
        assert!(c.sleeping, "already pending is not a new event");
        c.write32(0xe000e280, 1 << 5).unwrap();
        c.write32(0xe000e200, 1 << 5).unwrap();
        c.step_sleep();
        assert!(
            !c.sleeping,
            "new pending edge must wake WFE even with IRQ disabled"
        );
    }
}
#[test]
fn disabled_pending_interrupt_does_not_wake_without_sevonpend() {
    let mut c = cpu();
    execute(&mut c, 0xbf20, false);
    assert!(c.sleeping);
    c.write32(0xe000e200, 1 << 5).unwrap();
    c.step_sleep();
    assert!(c.sleeping);
}
#[test]
fn sev_does_not_wake_wfi_or_lose_its_event() {
    let mut c = cpu();
    execute(&mut c, 0xbf30, false);
    assert!(c.sleeping);
    execute(&mut c, 0xbf40, false);
    c.step_sleep();
    assert!(c.sleeping, "WFI does not wake on events");
    c.sleeping = false;
    execute(&mut c, 0xbf20, false);
    assert!(!c.sleeping);
    execute(&mut c, 0xbf20, false);
    assert!(c.sleeping);
}

#[test]
fn masked_irq_wakes_wfi_but_not_wfe_without_sevonpend() {
    for opcode in [0xbf20, 0xbf30] {
        let mut c = cpu();
        execute(&mut c, 0xb672, false); // CPSID i
        c.write32(0xe000e100, 1 << 5).unwrap();
        execute(&mut c, opcode, false);
        assert!(c.sleeping);
        c.write32(0xe000e200, 1 << 5).unwrap();
        c.step_sleep();
        assert_eq!(c.sleeping, opcode == 0xbf20);
    }
}
#[test]
fn enabling_sevonpend_or_a_masked_irq_does_not_invent_a_new_pending_edge() {
    let mut c = cpu();
    execute(&mut c, 0xb672, false);
    c.write32(0xe000e200, 1 << 5).unwrap();
    c.write32(0xe000ed10, 1 << 4).unwrap();
    c.write32(0xe000e100, 1 << 5).unwrap();
    execute(&mut c, 0xbf20, false);
    assert!(c.sleeping);
    c.step_sleep();
    assert!(c.sleeping);
}
#[test]
fn reset_clears_the_event_and_both_sleep_modes() {
    let mut c = cpu();
    c.signal_event();
    c.reset().unwrap();
    execute(&mut c, 0xbf20, false);
    assert!(c.sleeping);
    c.reset().unwrap();
    assert!(!c.sleeping);
    execute(&mut c, 0xbf20, false);
    assert!(c.sleeping);
}
#[test]
fn skipped_sev_does_not_create_an_event() {
    for compiled in [false, true] {
        let mut c = cpu();
        c.psr.value = 0x51050000;
        execute(&mut c, 0xbf18, compiled);
        execute(&mut c, 0xbf40, compiled);
        execute(&mut c, 0xbf20, compiled);
        assert!(c.sleeping);
    }
}
#[test]
fn wide_and_narrow_event_instructions_share_the_latch() {
    for compiled in [false, true] {
        for wide in [false, true] {
            let mut c = cpu();
            for op in [4, 2, 2] {
                let code = if wide {
                    ThumbCode::Thumb32 {
                        opcode: 0xf3af8000 | op,
                    }
                } else {
                    ThumbCode::Thumb16 {
                        opcode: 0xbf00 | ((op as u16) << 4),
                    }
                };
                let instruction = c.decode(code);
                if compiled {
                    c.execute_compiled(&instruction, if wide { 4 } else { 2 });
                } else {
                    c.execute(&instruction, if wide { 4 } else { 2 });
                }
            }
            assert!(c.sleeping);
        }
    }
}
#[test]
fn preemptible_pending_irq_latches_an_event_before_later_masking() {
    let mut c = cpu();
    c.write32(0xe000e100, 1 << 5).unwrap();
    c.write32(0xe000e200, 1 << 5).unwrap();
    execute(&mut c, 0xb672, false);
    execute(&mut c, 0xbf20, false);
    assert!(!c.sleeping);
    execute(&mut c, 0xbf20, false);
    assert!(c.sleeping);
}
