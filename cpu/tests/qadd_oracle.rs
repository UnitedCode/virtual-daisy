#![cfg(any(feature = "armv7m", feature = "armv7em"))]
use zmu_cortex_m::{
    Processor,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
};
const OPCODES: [u32; 4] = [0xfa82f081, 0xfa82f0a1, 0xfa82f091, 0xfa82f0b1];
#[cfg(feature = "has-dsp-ext")]
#[path = "../fixtures/qadd_probe_cases.rs"]
mod probe_cases;
#[test]
fn saturating_word_arithmetic_requires_dsp() {
    let cpu = Processor::new();
    for opcode in OPCODES {
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
    const DATA: &[u8] = include_bytes!("../fixtures/isa_oracle_qadd.bin");
    const FLAGS: u32 = 0xf80f0000;
    #[test]
    fn standalone_guest_cases_are_pinned_to_independent_word_records() {
        for [opcode, input, control, result, status] in probe_cases::WORD_CASES {
            let operation = OPCODES.iter().position(|&op| op == opcode).unwrap() as u32;
            assert!(DATA.as_chunks::<28>().0.iter().any(|record| {
                let row = words(record);
                row[..6] == [operation, input, input, control, result, status]
            }));
        }
    }
    fn words(record: &[u8]) -> [u32; 7] {
        core::array::from_fn(|i| u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap()))
    }
    #[test]
    fn all_results_and_flags_match_independently_executed_arm_instructions() {
        assert_eq!(DATA.len(), 8192 * 28);
        for compiled in [false, true] {
            let mut cpu = Processor::new();
            for record in DATA.as_chunks::<28>().0 {
                let [op, a, b, control, result, status, magic] = words(record);
                assert_eq!(magic, 0x44444151);
                let opcode = OPCODES[op as usize];
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
                    "op={op} a={a:08x} b={b:08x} control={control:08x}"
                );
                assert_eq!((cpu.get_r(Reg::R1), cpu.get_r(Reg::R2)), (a, b));
                assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
            }
        }
    }

    #[test]
    fn all_legal_register_triples_and_aliases_use_the_pinned_results() {
        let oracle: std::collections::BTreeMap<_, _> = DATA
            .as_chunks::<28>()
            .0
            .iter()
            .map(|record| {
                let [op, a, b, control, result, status, _] = words(record);
                ((op, a, b, control), (result, status))
            })
            .collect();
        let registers: Vec<u32> = (0..13).chain([14]).collect();
        for compiled in [false, true] {
            let mut cpu = Processor::new();
            for op in 0..4 {
                for (a, b, control) in [
                    (0x80000001, 0x40000000, 0xa00a0000),
                    (0x7fffffff, 0xbfffffff, 0x58050000),
                    (0, 0x80000000, 0x50050000),
                    (0x80000000, 0x80000000, 0xa80a0000),
                ] {
                    for &d in &registers {
                        for &m in &registers {
                            for &n in &registers {
                                let opcode =
                                    (OPCODES[op as usize] & !0x000f0f0f) | (n << 16) | (d << 8) | m;
                                let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                                cpu.r0_12 =
                                    core::array::from_fn(|i| 0xdeadbeef ^ (i as u32 * 0x01010101));
                                cpu.set_r(Reg::LR, 0xabcd1234);
                                cpu.set_r(Reg::from(m as u8), a);
                                cpu.set_r(Reg::from(n as u8), b);
                                cpu.psr.value = control | (1 << 24);
                                let before: [u32; 15] =
                                    core::array::from_fn(|i| cpu.get_r(Reg::from(i as u8)));
                                let expected =
                                    oracle[&(op, before[m as usize], before[n as usize], control)];
                                assert_eq!(
                                    zmu_cortex_m::core::instruction::instruction_size(&instruction),
                                    4
                                );
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
    fn doubled_operand_saturation_is_visible_even_when_the_final_result_fits() {
        for (op, a, b, result) in [
            (2, 0x80000001, 0x40000000, 0),
            (3, 0x80000000, 0x80000000, 0),
            (1, 0x80000000, 1, 0x80000000),
            (0, 0x7fffffff, 1, 0x7fffffff),
        ] {
            let record = DATA
                .as_chunks::<28>()
                .0
                .iter()
                .find(|record| {
                    let [o, x, y, c, _, _, _] = words(*record);
                    (o, x, y, c) == (op, a, b, 0xa00a0000)
                })
                .unwrap();
            let row = words(record);
            assert_eq!((row[4], row[5]), (result, 0xa80a0000));
        }
    }

    #[test]
    fn skipped_word_saturation_keeps_flags_and_operands() {
        for opcode in OPCODES {
            let mut cpu = Processor::new();
            cpu.r0_12.fill(0xdeadbeef);
            cpu.psr.value = 0x51050000;
            let it = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf18 });
            cpu.execute(&it, 2);
            let before = cpu.psr.value & FLAGS;
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            cpu.execute(&instruction, 4);
            assert_eq!(cpu.r0_12, [0xdeadbeef; 13]);
            assert_eq!(
                (cpu.psr.value & FLAGS, cpu.get_itstate(), cpu.cfsr, cpu.hfsr),
                (before, 0, 0, 0)
            );
        }
    }
}
