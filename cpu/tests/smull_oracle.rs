#![cfg(any(feature = "armv7m", feature = "armv7em"))]
use zmu_cortex_m::{
    Processor,
    core::{
        register::{BaseReg, Reg},
        thumb::ThumbCode,
    },
    decoder::Decoder,
    executor::Executor,
};
const DATA: &[u8] = include_bytes!("../fixtures/isa_oracle_smull.bin");
fn words(record: &[u8]) -> [u32; 8] {
    core::array::from_fn(|i| u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap()))
}
#[test]
fn negative_and_seeded_products_match_independent_arm_instructions() {
    assert_eq!(DATA.len(), 2048 * 32);
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for row in DATA.as_chunks::<32>().0 {
            let [opcode, a, b, control, lo, hi, status, magic] = words(row);
            assert_eq!(magic, 0x474e4f4c);
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
                (
                    cpu.get_r(Reg::R0),
                    cpu.get_r(Reg::R3),
                    cpu.psr.value & 0xf80f0000
                ),
                (lo, hi, status),
                "{row:x?}"
            );
            assert_eq!((cpu.get_r(Reg::R1), cpu.get_r(Reg::R2)), (a, b));
            assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
        }
    }
}
#[test]
fn all_legal_register_assignments_and_aliases_preserve_full_signed_products() {
    let oracle: std::collections::BTreeMap<_, _> = DATA
        .as_chunks::<32>()
        .0
        .iter()
        .map(|r| {
            let [_, a, b, c, lo, hi, status, _] = words(r);
            ((a, b, c), (lo, hi, status))
        })
        .collect();
    let registers: Vec<u32> = (0..13).chain([14]).collect();
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for (a, b) in [
            (0x80000000, 0x7fffffff),
            (0xffffffff, 1),
            (0xffffffff, 0xffffffff),
            (0x7fffffff, 0x80000000),
        ] {
            for &lo in &registers {
                for &hi in &registers {
                    if lo == hi {
                        continue;
                    }
                    for &n in &registers {
                        for &m in &registers {
                            let opcode = 0xfb800000 | (n << 16) | (lo << 12) | (hi << 8) | m;
                            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                            cpu.r0_12.fill(0xdeadbeef);
                            cpu.set_r(Reg::LR, 0xabcd1234);
                            cpu.set_r(Reg::from(n as u8), a);
                            cpu.set_r(Reg::from(m as u8), b);
                            cpu.psr.value = 0xa90a0000;
                            let before: [u32; 15] =
                                core::array::from_fn(|i| cpu.get_r(Reg::from(i as u8)));
                            let expected =
                                oracle[&(before[n as usize], before[m as usize], 0xa80a0000)];
                            if compiled {
                                cpu.execute_compiled(&instruction, 4);
                            } else {
                                cpu.execute(&instruction, 4);
                            }
                            assert_eq!(
                                (
                                    cpu.get_r(Reg::from(lo as u8)),
                                    cpu.get_r(Reg::from(hi as u8)),
                                    cpu.psr.value & 0xf80f0000
                                ),
                                expected,
                                "{opcode:08x}"
                            );
                            for (i, old) in before.into_iter().enumerate() {
                                if i != lo as usize && i != hi as usize {
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
