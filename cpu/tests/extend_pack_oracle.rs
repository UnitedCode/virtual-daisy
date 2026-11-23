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
const DATA: &[u8] = include_bytes!("../fixtures/isa_oracle_extend_pack.bin");
#[path = "../fixtures/extend_pack_probe_cases.rs"]
mod probe_cases;
#[test]
fn standalone_guest_cases_are_pinned_to_independent_records() {
    for [opcode, input, control, result, status] in probe_cases::EXTEND_PACK_CASES {
        let base = if unary(opcode) {
            (opcode & !15) | 2
        } else {
            (opcode & !0x000f000f) | 0x00010002
        };
        assert!(
            DATA.as_chunks::<28>()
                .0
                .iter()
                .any(|row| { words(row)[..6] == [base, input, input, control, result, status] }),
            "{opcode:08x}"
        );
    }
}
fn words(record: &[u8]) -> [u32; 7] {
    core::array::from_fn(|i| u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap()))
}
fn opcodes() -> Vec<u32> {
    DATA.as_chunks::<28>()
        .0
        .iter()
        .take(448)
        .step_by(4)
        .map(|r| words(r)[0])
        .collect()
}
fn unary(opcode: u32) -> bool {
    opcode >> 24 == 0xfa && ((opcode >> 16) & 15) == 15
}
fn available(opcode: u32) -> bool {
    cfg!(feature = "has-dsp-ext") || (unary(opcode) && matches!((opcode >> 20) & 7, 0 | 1 | 4 | 5))
}
#[test]
fn extensions_and_packing_obey_the_selected_profile() {
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
    assert_eq!(DATA.len(), 229376 * 28);
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for row in DATA.as_chunks::<28>().0 {
            let [opcode, a, b, control, result, status, magic] = words(row);
            assert_eq!(magic, 0x50545845);
            if !available(opcode) {
                continue;
            }
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            assert!(
                !matches!(instruction, Instruction::UDF { .. }),
                "missing {opcode:08x}"
            );
            cpu.set_r(Reg::R0, 0xdeadbeef);
            cpu.set_r(Reg::R1, a);
            cpu.set_r(Reg::R2, b);
            cpu.psr.value = control | 1 << 24;
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_eq!(
                (cpu.get_r(Reg::R0), cpu.psr.value & 0xf80f0000),
                (result, status),
                "{row:x?}"
            );
            assert_eq!((cpu.get_r(Reg::R1), cpu.get_r(Reg::R2)), (a, b));
            assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
        }
    }
}
#[test]
fn all_legal_register_triples_pairs_and_aliases_match_independent_results() {
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
        for base in opcodes().into_iter().filter(|&op| available(op)) {
            for (a, b, control) in [
                (0x7fffffff, 0x80000000, 0xa00a0000),
                (0x0000ffff, 0xffff0000, 0x58050000),
                (0x7fff8000, 0x80007fff, 0xa80a0000),
                (0xffffffff, 0x00ff00ff, 0x50050000),
            ] {
                for &d in &registers {
                    for &m in &registers {
                        for &n in if unary(base) {
                            &[15]
                        } else {
                            registers.as_slice()
                        } {
                            let opcode = (base & !0x000f0f0f) | (n << 16) | (d << 8) | m;
                            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                            cpu.r0_12 =
                                core::array::from_fn(|i| 0xdeadbeef ^ (i as u32 * 0x01010101));
                            cpu.set_r(Reg::LR, 0xabcd1234);
                            if n != 15 {
                                cpu.set_r(Reg::from(n as u8), a);
                            }
                            cpu.set_r(Reg::from(m as u8), b);
                            cpu.psr.value = control | 1 << 24;
                            let before: [u32; 15] =
                                core::array::from_fn(|i| cpu.get_r(Reg::from(i as u8)));
                            let actual_a = if n == 15 { 0 } else { before[n as usize] };
                            let expected = oracle[&(base, actual_a, before[m as usize], control)];
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
fn skipped_extensions_and_packing_keep_registers_and_flags() {
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
