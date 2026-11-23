#![cfg(any(feature = "armv7m", feature = "armv7em"))]
use zmu_cortex_m::{Processor, bus::Bus};

#[test]
fn m7_variants_keep_read_only_ccr_bits_and_ignore_reserved_writes() {
    for cpuid in [0x410f_c270, 0x411f_c271, 0x412f_c272] {
        let mut cpu = Processor::new();
        cpu.cpuid = cpuid;
        for (value, expected) in [(0, 0x40200), (u32::MAX, 0x7031b), (0x40000, 0x40200)] {
            cpu.write32(0xe000_ed14, value).unwrap();
            assert_eq!(cpu.read32(0xe000_ed14).unwrap(), expected);
        }
    }
}

#[test]
fn other_cpu_profiles_keep_their_configurable_stack_alignment() {
    for cpuid in [0, 0x412f_c231, 0x410f_c241, 0x510f_c270] {
        let mut cpu = Processor::new();
        cpu.cpuid = cpuid;
        cpu.write32(0xe000_ed14, 0x200).unwrap();
        assert_eq!(cpu.read32(0xe000_ed14).unwrap(), 0x200);
        cpu.write32(0xe000_ed14, 0).unwrap();
        assert_eq!(cpu.read32(0xe000_ed14).unwrap(), 0);
    }
}
