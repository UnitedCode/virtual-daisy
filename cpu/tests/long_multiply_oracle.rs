#![cfg(any(feature = "armv7m", feature = "armv7em"))]
use zmu_cortex_m::{
    Processor,
    core::{
        instruction::Instruction,
        register::{BaseReg, Reg},
        thumb::ThumbCode,
    },
    decoder::Decoder,
    executor::Executor,
};
const DATA: &[u8] = include_bytes!("../fixtures/isa_oracle_long_multiply.bin");
#[path = "../fixtures/long_multiply_probe_cases.rs"]
mod probe_cases;
#[test]
fn standalone_guest_cases_are_pinned_to_independent_records() {
    for [opcode, input, control, lo, hi, status] in probe_cases::LONG_CASES {
        let base = (opcode & !0x000f000f) | 0x00010002;
        assert!(DATA.as_chunks::<40>().0.iter().any(|record| {
            words(record)[..9] == [base, input, input, input, input, control, lo, hi, status]
        }));
    }
}
fn words(record: &[u8]) -> [u32; 10] {
    core::array::from_fn(|i| u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap()))
}
fn opcodes() -> Vec<u32> {
    DATA.as_chunks::<40>()
        .0
        .iter()
        .take(52)
        .step_by(4)
        .map(|r| words(r)[0])
        .collect()
}
fn available(opcode: u32) -> bool {
    cfg!(feature = "has-dsp-ext")
        || matches!(
            opcode & 0xfff000f0,
            0xfb800000 | 0xfba00000 | 0xfbc00000 | 0xfbe00000
        )
}
#[test]
fn long_arithmetic_obeys_the_selected_profile() {
    let cpu = Processor::new();
    for opcode in opcodes() {
        assert_eq!(
            !matches!(
                cpu.decode(ThumbCode::Thumb32 { opcode }),
                Instruction::UDF { .. }
            ),
            available(opcode),
            "{opcode:08x}"
        );
    }
}
#[test]
fn all_results_and_flags_match_independently_executed_arm_instructions() {
    assert_eq!(DATA.len(), 292864 * 40);
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for row in DATA.as_chunks::<40>().0 {
            let [opcode, a, b, acc_lo, acc_hi, control, lo, hi, status, magic] = words(row);
            assert_eq!(magic, 0x4d4c4c44);
            if !available(opcode) {
                continue;
            }
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            assert!(
                !matches!(instruction, Instruction::UDF { .. }),
                "missing {opcode:08x}"
            );
            cpu.set_r(Reg::R0, acc_lo);
            cpu.set_r(Reg::R3, acc_hi);
            cpu.set_r(Reg::R1, a);
            cpu.set_r(Reg::R2, b);
            cpu.psr.value = control | 1 << 24;
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
fn all_legal_register_assignments_and_aliases_match_independent_results() {
    let oracle: std::collections::BTreeMap<_, _> = DATA
        .as_chunks::<40>()
        .0
        .iter()
        .map(|r| {
            let [op, a, b, al, ah, c, lo, hi, status, _] = words(r);
            ((op, a, b, al, ah, c), (lo, hi, status))
        })
        .collect();
    let registers: Vec<u32> = (0..13).chain([14]).collect();
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for base in opcodes().into_iter().filter(|&op| available(op)) {
            for (a, b, initial) in [
                (0x7fffffff, 0x80000000, 0),
                (0x80000000, 0x7fffffff, 0x80000000),
            ] {
                for &lo in &registers {
                    for &hi in &registers {
                        if lo == hi {
                            continue;
                        }
                        for &n in &registers {
                            for &m in &registers {
                                let opcode =
                                    (base & !0x000fff0f) | (n << 16) | (lo << 12) | (hi << 8) | m;
                                let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                                cpu.r0_12.fill(0xdeadbeef);
                                cpu.set_r(Reg::LR, 0xabcd1234);
                                cpu.set_r(Reg::from(lo as u8), initial);
                                cpu.set_r(Reg::from(hi as u8), initial);
                                cpu.set_r(Reg::from(n as u8), a);
                                cpu.set_r(Reg::from(m as u8), b);
                                cpu.psr.value = 0xa90a0000;
                                let before: [u32; 15] =
                                    core::array::from_fn(|i| cpu.get_r(Reg::from(i as u8)));
                                let expected = oracle[&(
                                    base,
                                    before[n as usize],
                                    before[m as usize],
                                    before[lo as usize],
                                    before[hi as usize],
                                    0xa80a0000,
                                )];
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
}
#[test]
fn skipped_long_arithmetic_preserves_registers_and_flags() {
    for compiled in [false, true] {
        for opcode in opcodes().into_iter().filter(|&op| available(op)) {
            let mut cpu = Processor::new();
            cpu.r0_12.fill(0xdeadbeef);
            cpu.psr.value = 0x51050000;
            let it = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf18 });
            cpu.execute(&it, 2);
            let before = cpu.psr.value & 0xf80f0000;
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_eq!(cpu.r0_12, [0xdeadbeef; 13]);
            assert_eq!(
                (
                    cpu.psr.value & 0xf80f0000,
                    cpu.get_itstate(),
                    cpu.cfsr,
                    cpu.hfsr
                ),
                (before, 0, 0, 0)
            );
        }
    }
}
