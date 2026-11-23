#[cfg(feature = "has-fp")]
use zmu_cortex_m::core::register::{DoubleReg, ExtensionRegOperations};
use zmu_cortex_m::{
    Processor,
    bus::Bus,
    core::{
        register::{BaseReg, Reg},
        thumb::ThumbCode,
    },
    decoder::Decoder,
    executor::Executor,
};

fn fixture() -> (Processor, Vec<u8>) {
    let mut bytes: Vec<_> = (0..2048u32)
        .map(|i| (i.wrapping_mul(73) ^ (i >> 3) ^ 0xa5) as u8)
        .collect();
    for (at, value) in [
        (0, 0x2001fff0u32),
        (4, 0x101),
        (12, 0xc1),
        (16, 0xc1),
        (24, 0xc1),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut cpu = Processor::new();
    cpu.flash_memory(bytes.len(), &bytes);
    cpu.msp = 0x2001fff0;
    cpu.cpacr = 0x00f00000;
    cpu.branch_write_pc(0x100);
    (cpu, bytes)
}
fn execute(cpu: &mut Processor, raw: u32, wide: bool, compiled: bool) {
    let instruction = cpu.decode(if wide {
        ThumbCode::Thumb32 { opcode: raw }
    } else {
        ThumbCode::Thumb16 { opcode: raw as u16 }
    });
    let size = if wide { 4 } else { 2 };
    if compiled {
        cpu.execute_compiled(&instruction, size);
    } else {
        cpu.execute(&instruction, size);
    }
}

#[test]
fn negative_literal_addresses_wrap_before_delivering_a_memory_fault() {
    for compiled in [false, true] {
        let (mut cpu, _) = fixture();
        cpu.r0_12[4] = 0xdeadbeef;
        execute(&mut cpu, 0xf85f4108, true, compiled); // Align(0x104,4)-0x108 = 0xfffffffc.
        assert_eq!(cpu.cfsr & 0x82, 0x82);
        assert_eq!(cpu.read32(0xe000ed34).unwrap(), 0xfffffffc);
        assert_eq!(cpu.r0_12[4], 0xdeadbeef);
        assert_eq!(cpu.get_r(Reg::PC), 0xc4);
    }
}

#[cfg(feature = "has-fp")]
#[test]
fn floating_loads_wrap_into_populated_memory_for_every_register_and_offset() {
    for compiled in [false, true] {
        let (mut cpu, bytes) = fixture();
        for double in [false, true] {
            for register in 0..if double { 16u32 } else { 32u32 } {
                for immediate in 1..=255u32 {
                    cpu.branch_write_pc(0x100);
                    cpu.psr.value = 0xa90a0000;
                    cpu.r0_12[1] = 0xfffffffc;
                    let encoded = if double {
                        0xed910b00 | (register << 12)
                    } else {
                        0xed910a00 | ((register >> 1) << 12) | ((register & 1) << 22)
                    } | immediate;
                    let address = (immediate * 4 - 4) as usize;
                    execute(&mut cpu, encoded, true, compiled);
                    assert_eq!(cpu.cfsr, 0, "{encoded:08x}");
                    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                    if double {
                        assert_eq!(
                            cpu.get_dr(DoubleReg::from(register as u8)),
                            (word(address), word(address + 4))
                        );
                    } else {
                        assert_eq!(cpu.fp_regs[register as usize], word(address));
                    }
                    assert_eq!(cpu.r0_12[1], 0xfffffffc);
                    assert_eq!(cpu.psr.value, 0xa90a0000);
                }
            }
        }
    }
}

#[cfg(feature = "has-fp")]
#[test]
fn floating_loads_and_stores_report_wrapped_unmapped_addresses_without_panicking() {
    for compiled in [false, true] {
        for opcode in [0xed119a01, 0xed117b01, 0xed019a01, 0xed017b01] {
            let (mut cpu, _) = fixture();
            cpu.r0_12[1] = 0;
            cpu.fp_regs.fill(0x12345678);
            execute(&mut cpu, opcode, true, compiled);
            assert_eq!(cpu.cfsr & 0x82, 0x82, "{opcode:08x}");
            assert_eq!(cpu.read32(0xe000ed34).unwrap(), 0xfffffffc);
            assert_eq!(cpu.fp_regs, [0x12345678; 32]);
            assert_eq!(cpu.r0_12[1], 0);
        }
    }
}

#[test]
fn decrement_before_stores_deliver_faults_without_changing_operands_or_writeback() {
    for compiled in [false, true] {
        for wback in [false, true] {
            let (mut cpu, _) = fixture();
            cpu.r0_12[0] = 0x12345678;
            cpu.r0_12[1] = 0xabcdef01;
            cpu.r0_12[8] = 0;
            execute(
                &mut cpu,
                0xe9080003 | (u32::from(wback) << 21),
                true,
                compiled,
            );
            assert_eq!(cpu.cfsr & 0x82, 0x82);
            assert_eq!(cpu.read32(0xe000ed34).unwrap(), 0xfffffff8);
            assert_eq!(&cpu.r0_12[..2], &[0x12345678, 0xabcdef01]);
            assert_eq!(cpu.r0_12[8], 0);
        }
    }
}

#[cfg(feature = "has-fp")]
#[test]
fn floating_store_multiple_wraps_its_decrement_and_reports_the_failed_access() {
    for compiled in [false, true] {
        for opcode in [0xed2adb06, 0xed2ada06] {
            let (mut cpu, _) = fixture();
            cpu.r0_12[10] = 0;
            cpu.fp_regs.fill(0x12345678);
            execute(&mut cpu, opcode, true, compiled);
            assert_eq!(cpu.cfsr & 0x82, 0x82);
            assert_eq!(cpu.read32(0xe000ed34).unwrap(), 0xffffffe8);
            assert_eq!(cpu.fp_regs, [0x12345678; 32]);
        }
    }
}

#[test]
fn invalid_stack_extents_raise_guest_faults_without_a_host_panic() {
    for compiled in [false, true] {
        for (opcode, wide, sp) in [(0xb401, false, 0), (0xbc01, false, 0xfffffffc)] {
            let (mut cpu, _) = fixture();
            cpu.msp = sp;
            execute(&mut cpu, opcode, wide, compiled);
            assert_ne!(cpu.cfsr, 0);
        }
    }
}

#[cfg(feature = "has-fp")]
#[test]
fn floating_stack_extents_raise_guest_faults_without_a_host_panic() {
    for compiled in [false, true] {
        for (opcode, sp) in [(0xed2d0a02, 0), (0xecbd0a02, 0xfffffffc)] {
            let (mut cpu, _) = fixture();
            cpu.msp = sp;
            execute(&mut cpu, opcode, true, compiled);
            assert_ne!(cpu.cfsr, 0);
        }
    }
}

#[test]
fn register_and_pc_increment_helpers_preserve_32_bit_wrapping() {
    let (mut cpu, _) = fixture();
    for bank in [false, true] {
        cpu.r0_12[0] = u32::from(bank) << 1;
        execute(&mut cpu, 0xf3808814, true, false); // MSR CONTROL,R0 selects the bank.
        for register in 0..15u8 {
            let r = Reg::from(register);
            cpu.set_r(r, 0xfffffff0);
            cpu.add_r(r, 0x20);
            assert_eq!(cpu.get_r(r), 0x10);
            cpu.sub_r(r, 0x20);
            assert_eq!(cpu.get_r(r), 0xfffffff0);
        }
    }
    cpu.branch_write_pc(0xfffffffc);
    assert_eq!(cpu.get_r(Reg::PC), 0);
    cpu.add_pc(4);
    assert_eq!(cpu.get_pc(), 0);
    cpu.sub_r(Reg::PC, 4);
    assert_eq!(cpu.get_pc(), 0xfffffffc);
    cpu.add_r(Reg::PC, 4);
    assert_eq!(cpu.get_pc(), 0);
}
