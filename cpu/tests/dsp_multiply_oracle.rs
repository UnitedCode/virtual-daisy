#![cfg(any(feature = "armv7m", feature = "armv7em"))]
use zmu_cortex_m::{
    Processor,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
};
const DATA: &[u8] = include_bytes!("../fixtures/isa_oracle_dsp_multiply.bin");
#[cfg(feature = "has-dsp-ext")]
#[path = "../fixtures/dsp_multiply_probe_cases.rs"]
mod probe_cases;
fn words(record: &[u8]) -> [u32; 8] {
    core::array::from_fn(|i| u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap()))
}
fn opcodes() -> Vec<u32> {
    DATA.as_chunks::<32>()
        .0
        .iter()
        .take(112)
        .step_by(4)
        .map(|r| words(r)[0])
        .collect()
}
#[test]
fn multiply_and_absolute_difference_require_dsp() {
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
#[cfg(feature = "has-dsp-ext")]
mod arithmetic {
    use super::*;
    use zmu_cortex_m::{
        core::register::{BaseReg, Reg},
        executor::Executor,
    };
    const FLAGS: u32 = 0xf80f0000;
    #[test]
    fn standalone_guest_cases_are_pinned_to_independent_records() {
        for [opcode, input, control, result, status] in probe_cases::DSP_MULTIPLY_CASES {
            let base = (opcode & !0x000f000f) | 0x00010002;
            assert!(DATA.as_chunks::<32>().0.iter().any(|record| {
                words(record)[..7] == [base, input, input, input, control, result, status]
            }));
        }
    }
    #[test]
    fn all_results_and_flags_match_independently_executed_arm_instructions() {
        assert_eq!(DATA.len(), 229376 * 32);
        for compiled in [false, true] {
            let mut cpu = Processor::new();
            for row in DATA.as_chunks::<32>().0 {
                let [opcode, a, b, c, control, result, status, magic] = words(row);
                assert_eq!(magic, 0x504c554d);
                let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                assert!(
                    !matches!(instruction, Instruction::UDF { .. }),
                    "missing {opcode:08x}"
                );
                cpu.set_r(Reg::R0, 0xdeadbeef);
                cpu.set_r(Reg::R1, a);
                cpu.set_r(Reg::R2, b);
                cpu.set_r(Reg::R3, c);
                cpu.psr.value = control | 1 << 24;
                if compiled {
                    cpu.execute_compiled(&instruction, 4);
                } else {
                    cpu.execute(&instruction, 4);
                }
                assert_eq!(
                    (cpu.get_r(Reg::R0), cpu.psr.value & FLAGS),
                    (result, status),
                    "opcode={opcode:08x} a={a:08x} b={b:08x} c={c:08x} control={control:08x}"
                );
                assert_eq!(
                    (cpu.get_r(Reg::R1), cpu.get_r(Reg::R2), cpu.get_r(Reg::R3)),
                    (a, b, c)
                );
                assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
            }
        }
    }
    #[test]
    fn all_legal_registers_and_accumulator_aliases_match_independent_results() {
        let oracle: std::collections::BTreeMap<_, _> = DATA
            .as_chunks::<32>()
            .0
            .iter()
            .map(|r| {
                let [op, a, b, c, flags, result, status, _] = words(r);
                ((op, a, b, c, flags), (result, status))
            })
            .collect();
        let registers: Vec<u32> = (0..13).chain([14]).collect();
        for compiled in [false, true] {
            let mut cpu = Processor::new();
            for base in opcodes() {
                let accumulated = (base >> 12) & 15 != 15;
                for &d in &registers {
                    for &n in &registers {
                        for &m in &registers {
                            for &acc in if accumulated {
                                registers.as_slice()
                            } else {
                                &[15]
                            } {
                                let opcode =
                                    (base & !0x000fff0f) | (n << 16) | (acc << 12) | (d << 8) | m;
                                let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                                cpu.r0_12 =
                                    core::array::from_fn(|i| 0xdeadbeef ^ (i as u32 * 0x01010101));
                                cpu.set_r(Reg::LR, 0xabcd1234);
                                cpu.set_r(Reg::from(n as u8), 0x7fffffff);
                                cpu.set_r(Reg::from(m as u8), 0x80000000);
                                if accumulated {
                                    cpu.set_r(Reg::from(acc as u8), 1);
                                }
                                cpu.psr.value = 0xa90a0000;
                                let before: [u32; 15] =
                                    core::array::from_fn(|i| cpu.get_r(Reg::from(i as u8)));
                                let actual_acc = if accumulated { before[acc as usize] } else { 0 };
                                let expected = oracle[&(
                                    base,
                                    before[n as usize],
                                    before[m as usize],
                                    actual_acc,
                                    0xa80a0000,
                                )];
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
    fn skipped_multiply_preserves_flags_and_registers() {
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
