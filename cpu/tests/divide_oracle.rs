#![cfg(any(feature = "armv7m", feature = "armv7em"))]
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
const DATA: &[u8] = include_bytes!("../fixtures/isa_oracle_divide.bin");
#[path = "../fixtures/divide_probe_cases.rs"]
mod probe_cases;
#[test]
fn architectural_guest_divisions_are_pinned_to_independent_records() {
    for case in probe_cases::CPU_DIVIDE_CASES {
        assert!(
            DATA.as_chunks::<28>()
                .0
                .iter()
                .any(|row| words(row)[..6] == case)
        );
    }
}
fn words(row: &[u8]) -> [u32; 7] {
    core::array::from_fn(|i| u32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().unwrap()))
}
#[test]
fn all_results_and_flags_match_independently_executed_divisions() {
    assert_eq!(DATA.len(), 4096 * 28);
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for row in DATA.as_chunks::<28>().0 {
            let [opcode, a, b, control, result, status, magic] = words(row);
            assert_eq!(magic, 0x49564944);
            cpu.set_r(Reg::R0, 0xdeadbeef);
            cpu.set_r(Reg::R1, a);
            cpu.set_r(Reg::R2, b);
            cpu.psr.value = control | 1 << 24;
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_eq!(
                (cpu.get_r(Reg::R0), cpu.psr.value & 0xf80f0000),
                (result, status),
                "{opcode:08x} {a:x}/{b:x}"
            );
            assert_eq!((cpu.get_r(Reg::R1), cpu.get_r(Reg::R2)), (a, b));
            assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
        }
    }
}
#[test]
fn all_legal_register_triples_and_aliases_match_independent_divisions() {
    let oracle: std::collections::BTreeMap<_, _> = DATA
        .as_chunks::<28>()
        .0
        .iter()
        .map(|r| {
            let [op, a, b, c, result, status, _] = words(r);
            ((op, a, b, c), (result, status))
        })
        .collect();
    let regs: Vec<u32> = (0..13).chain([14]).collect();
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for base in [0xfb91f0f2, 0xfbb1f0f2] {
            for (a, b, control) in [
                (0x80000000, 0xffffffff, 0xa80a0000),
                (0xfffffff9, 3, 0x58050000),
                (0xffffffff, 3, 0xa00a0000),
                (0x12345678, 0, 0x50050000),
            ] {
                for &d in &regs {
                    for &n in &regs {
                        for &m in &regs {
                            let opcode = (base & !0x000f0f0f) | (n << 16) | (d << 8) | m;
                            cpu.r0_12.fill(0xdeadbeef);
                            cpu.set_r(Reg::LR, 0xabcd1234);
                            cpu.set_r(Reg::from(n as u8), a);
                            cpu.set_r(Reg::from(m as u8), b);
                            let before: [u32; 15] =
                                core::array::from_fn(|i| cpu.get_r(Reg::from(i as u8)));
                            let expected =
                                oracle[&(base, before[n as usize], before[m as usize], control)];
                            cpu.psr.value = control | 1 << 24;
                            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                            if compiled {
                                cpu.execute_compiled(&instruction, 4);
                            } else {
                                cpu.execute(&instruction, 4);
                            }
                            assert_eq!(
                                (cpu.get_r(Reg::from(d as u8)), cpu.psr.value & 0xf80f0000),
                                expected,
                                "{opcode:08x}"
                            );
                            for (i, old) in before.into_iter().enumerate() {
                                if i != d as usize {
                                    assert_eq!(cpu.get_r(Reg::from(i as u8)), old);
                                }
                            }
                            assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn skipped_zero_divisors_do_not_fault_when_trapping_is_enabled() {
    for compiled in [false, true] {
        for opcode in [0xfb91f0f2, 0xfbb1f0f2] {
            let mut cpu = Processor::new();
            cpu.write32(0xe000ed14, 0x210).unwrap();
            cpu.psr.value = 0x51050000;
            cpu.set_r(Reg::R0, 0xdeadbeef);
            cpu.set_r(Reg::R2, 0);
            let it = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf18 });
            cpu.execute(&it, 2);
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_eq!(cpu.get_r(Reg::R0), 0xdeadbeef);
            assert_eq!((cpu.cfsr, cpu.hfsr, cpu.get_itstate()), (0, 0, 0));
        }
    }
}
#[test]
fn zero_divisors_trap_without_writing_the_destination() {
    for compiled in [false, true] {
        for opcode in [0xfb91f0f2, 0xfbb1f0f2] {
            let mut cpu = Processor::new();
            cpu.write32(0xe000ed14, 0x210).unwrap();
            cpu.psr.value = 0xa90a0000;
            cpu.set_r(Reg::SP, 0x2001f000);
            cpu.set_r(Reg::R0, 0xdeadbeef);
            cpu.set_r(Reg::R2, 0);
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_eq!(cpu.get_r(Reg::R0), 0xdeadbeef);
            assert_eq!(cpu.cfsr & 0x02010000, 1 << 25);
            assert_eq!(cpu.psr.value & 0xf80f0000, 0xa80a0000);
        }
    }
}
