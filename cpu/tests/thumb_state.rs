#![cfg(any(feature = "armv7m", feature = "armv7em"))]
use zmu_cortex_m::{
    Processor,
    bus::Bus,
    core::register::{BaseReg, Epsr, Ipsr},
    executor::Executor,
};

// Keep the always-inlined debug decoder in one call frame.
#[inline(never)]
fn step(cpu: &mut Processor) {
    cpu.step();
}

fn fixture(opcode: u32, wide: bool, cached: bool, target: u32, enabled: bool) -> Processor {
    let mut bytes = [0u8; 512];
    for (at, value) in [(0, 0x2001fff0u32), (4, 0x101), (12, 0x181), (24, 0x1a1)] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    if wide {
        bytes[0x100..0x102].copy_from_slice(&((opcode >> 16) as u16).to_le_bytes());
        bytes[0x102..0x104].copy_from_slice(&(opcode as u16).to_le_bytes());
    } else {
        bytes[0x100..0x102].copy_from_slice(&(opcode as u16).to_le_bytes());
    }
    bytes[0x140..0x142].copy_from_slice(&0x2255u16.to_le_bytes()); // MOVS R2,#0x55.
    let mut cpu = Processor::new();
    cpu.flash_memory(bytes.len(), &bytes);
    cpu.msp = 0x2001fff0;
    cpu.psr.value = 1 << 24;
    cpu.r0_12[2] = 0xdeadbeef;
    cpu.branch_write_pc(0x100);
    match opcode {
        0x4700 | 0x4780 => cpu.r0_12[0] = target,
        0xf8d0f000 => {
            cpu.r0_12[0] = 0x20000040;
            cpu.write32(0x20000040, target).unwrap();
        }
        0xbd00 => {
            cpu.write32(cpu.msp, target).unwrap();
        }
        0xe8b08008 => {
            cpu.r0_12[0] = 0x20000040;
            cpu.write32(0x20000040, 0x12345678).unwrap();
            cpu.write32(0x20000044, target).unwrap();
        }
        _ => unreachable!(),
    }
    if enabled {
        cpu.write32(0xe000ed24, 1 << 18).unwrap();
    }
    if cached {
        cpu.cache_instructions();
        // Populate the target's decoded slot while execution state is valid.
        cpu.branch_write_pc(0x140);
        step(&mut cpu);
        cpu.r0_12[2] = 0xdeadbeef;
        cpu.psr.value = 1 << 24;
        cpu.branch_write_pc(0x100);
    }
    cpu
}

#[test]
fn even_interworking_targets_fault_on_the_target_instruction_before_side_effects() {
    for cached in [false, true] {
        for enabled in [false, true] {
            for (opcode, wide) in [
                (0x4700, false),
                (0x4780, false),
                (0xf8d0f000, true),
                (0xbd00, false),
                (0xe8b08008, true),
            ] {
                let mut cpu = fixture(opcode, wide, cached, 0x140, enabled);
                step(&mut cpu);
                assert_eq!(cpu.get_pc(), 0x140);
                assert!(!cpu.psr.get_t());
                assert_eq!(cpu.cfsr, 0);
                step(&mut cpu);
                assert_eq!(
                    cpu.cfsr,
                    1 << 17,
                    "opcode={opcode:08x}, cached={cached}, enabled={enabled}"
                );
                assert_eq!(cpu.hfsr, if enabled { 0 } else { 1 << 30 });
                assert_eq!(cpu.psr.get_isr_number(), if enabled { 6 } else { 3 });
                assert!(cpu.psr.get_t());
                assert_eq!(cpu.r0_12[2], 0xdeadbeef);
                assert_eq!(cpu.read32(cpu.msp + 24).unwrap(), 0x140);
                assert_eq!(cpu.read32(cpu.msp + 28).unwrap() & (1 << 24), 0);
            }
        }
    }
}

#[test]
fn odd_interworking_targets_execute_the_target_without_faults() {
    for cached in [false, true] {
        for (opcode, wide) in [
            (0x4700, false),
            (0x4780, false),
            (0xf8d0f000, true),
            (0xbd00, false),
            (0xe8b08008, true),
        ] {
            let mut cpu = fixture(opcode, wide, cached, 0x141, true);
            step(&mut cpu);
            step(&mut cpu);
            assert_eq!(cpu.cfsr, 0, "{opcode:08x}");
            assert_eq!(cpu.hfsr, 0);
            assert_eq!(cpu.psr.get_isr_number(), 0);
            assert!(cpu.psr.get_t());
            assert_eq!(cpu.r0_12[2], 0x55);
            assert_eq!(cpu.get_pc(), 0x142);
        }
    }
}
