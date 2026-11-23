#![cfg(any(feature = "armv7m", feature = "armv7em"))]
use zmu_cortex_m::{
    Processor,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
};
const DATA: &[u8] = include_bytes!("../fixtures/isa_oracle_parallel.bin");
#[cfg(feature = "has-dsp-ext")]
#[path = "../fixtures/parallel_probe_cases.rs"]
mod probe_cases;
fn words(record: &[u8]) -> [u32; 7] {
    core::array::from_fn(|i| u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap()))
}
fn opcodes() -> Vec<u32> {
    DATA.as_chunks::<28>()
        .0
        .iter()
        .take(144)
        .step_by(4)
        .map(|r| words(r)[0])
        .collect()
}
#[test]
fn packed_arithmetic_requires_dsp() {
    let cpu = Processor::new();
    for opcode in opcodes() {
        assert_eq!(
            !matches!(
                cpu.decode(ThumbCode::Thumb32 { opcode }),
                Instruction::UDF { .. }
            ),
            cfg!(feature = "has-dsp-ext"),
            "{opcode:08x}"
        );
    }
}
#[test]
fn reserved_packed_arithmetic_encodings_remain_undefined() {
    let cpu = Processor::new();
    for operation in 8..16 {
        for flavor in 0..8 {
            if matches!(operation, 8 | 9 | 10 | 12 | 13 | 14)
                && matches!(flavor, 0 | 1 | 2 | 4 | 5 | 6)
            {
                continue;
            }
            let opcode = 0xfa01f002 | (operation << 20) | (flavor << 4);
            assert!(
                matches!(
                    cpu.decode(ThumbCode::Thumb32 { opcode }),
                    Instruction::UDF { .. }
                ),
                "{opcode:08x}"
            );
        }
    }
}
#[cfg(feature = "has-dsp-ext")]
mod arithmetic {
    use super::*;
    use zmu_cortex_m::{
        core::register::{BaseReg, Reg},
        executor::Executor,
    };
    const FLAGS: u32 = 0xf80f0000;
    #[test]
    fn standalone_guest_cases_come_from_independent_records() {
        for [opcode, input, control, result, status] in probe_cases::PARALLEL_CASES {
            let base = (opcode & !0x000f000f) | 0x00010002;
            assert!(DATA.as_chunks::<28>().0.iter().any(|record| {
                words(record)[..6] == [base, input, input, control, result, status]
            }));
        }
    }
    #[test]
    fn all_results_and_flags_match_independently_executed_arm_instructions() {
        assert_eq!(DATA.len(), 73728 * 28);
        for compiled in [false, true] {
            let mut cpu = Processor::new();
            for record in DATA.as_chunks::<28>().0 {
                let [opcode, a, b, control, result, status, magic] = words(record);
                assert_eq!(magic, 0x504d4953);
                let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                assert!(
                    !matches!(instruction, Instruction::UDF { .. }),
                    "missing {opcode:08x}"
                );
                cpu.set_r(Reg::R0, 0xdeadbeef);
                cpu.set_r(Reg::R1, a);
                cpu.set_r(Reg::R2, b);
                cpu.psr.value = control | (1 << 24);
                if compiled {
                    cpu.execute_compiled(&instruction, 4);
                } else {
                    cpu.execute(&instruction, 4);
                }
                assert_eq!(
                    (cpu.get_r(Reg::R0), cpu.psr.value & FLAGS),
                    (result, status & FLAGS),
                    "opcode={opcode:08x} a={a:08x} b={b:08x} control={control:08x}"
                );
                assert_eq!((cpu.get_r(Reg::R1), cpu.get_r(Reg::R2)), (a, b));
                assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
            }
        }
    }
    #[test]
    fn all_legal_register_triples_and_aliases_use_independent_results() {
        let oracle: std::collections::BTreeMap<_, _> = DATA
            .as_chunks::<28>()
            .0
            .iter()
            .map(|r| {
                let [op, a, b, c, result, status, _] = words(r);
                ((op, a, b, c), (result, status))
            })
            .collect();
        let registers: Vec<u32> = (0..13).chain([14]).collect();
        for compiled in [false, true] {
            let mut cpu = Processor::new();
            for base in opcodes() {
                for (a, b, control) in [
                    (0x7fff7fff, 1, 0xa00a0000),
                    (0x80008000, 0xffffffff, 0x58050000),
                    (0x7fff8000, 0x80007fff, 0xa80a0000),
                    (0, 0xffffffff, 0x50050000),
                ] {
                    for &d in &registers {
                        for &n in &registers {
                            for &m in &registers {
                                let opcode = (base & !0x000f0f0f) | (n << 16) | (d << 8) | m;
                                let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                                cpu.r0_12 =
                                    core::array::from_fn(|i| 0xdeadbeef ^ (i as u32 * 0x01010101));
                                cpu.set_r(Reg::LR, 0xabcd1234);
                                cpu.set_r(Reg::from(n as u8), a);
                                cpu.set_r(Reg::from(m as u8), b);
                                cpu.psr.value = control | (1 << 24);
                                let before: [u32; 15] =
                                    core::array::from_fn(|i| cpu.get_r(Reg::from(i as u8)));
                                let expected = oracle
                                    [&(base, before[n as usize], before[m as usize], control)];
                                if compiled {
                                    cpu.execute_compiled(&instruction, 4);
                                } else {
                                    cpu.execute(&instruction, 4);
                                }
                                assert_eq!(
                                    (cpu.get_r(Reg::from(d as u8)), cpu.psr.value & FLAGS),
                                    expected,
                                    "{opcode:08x}"
                                );
                                for (i, old) in before.into_iter().enumerate() {
                                    if i != d as usize {
                                        assert_eq!(
                                            cpu.get_r(Reg::from(i as u8)),
                                            old,
                                            "{opcode:08x} changed r{i}"
                                        );
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
    fn skipped_packed_arithmetic_preserves_flags_and_registers() {
        for compiled in [false, true] {
            for opcode in opcodes() {
                let mut cpu = Processor::new();
                cpu.r0_12.fill(0xdeadbeef);
                cpu.psr.value = 0x51050000;
                let it = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf18 });
                cpu.execute(&it, 2);
                let before = cpu.psr.value & FLAGS;
                let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                if compiled {
                    cpu.execute_compiled(&instruction, 4);
                } else {
                    cpu.execute(&instruction, 4);
                }
                assert_eq!(cpu.r0_12, [0xdeadbeef; 13]);
                assert_eq!(
                    (cpu.psr.value & FLAGS, cpu.get_itstate(), cpu.cfsr, cpu.hfsr),
                    (before, 0, 0, 0)
                );
            }
        }
    }
}
