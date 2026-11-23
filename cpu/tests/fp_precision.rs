#![cfg(feature = "fpv5-d16")]
use zmu_cortex_m::{
    Processor,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
    executor::Executor,
};

#[test]
fn precision_conversions_match_executed_arm_results_and_flags() {
    let data = include_bytes!("../fixtures/isa_oracle_precision.bin");
    assert_eq!(data.len(), 32_768 * 28);
    for native in [false, true] {
        for compiled in [false, true] {
            for sticky in [0, 0x9f] {
                let mut cpu = Processor::new();
                cpu.cpacr = 0x00f00000;
                cpu.native_float = native;
                for record in data.as_chunks::<28>().0 {
                    let [direction, il, ih, control, lo, hi, status]: [u32; 7] =
                        core::array::from_fn(|i| {
                            u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
                        });
                    let opcode = if direction == 0 {
                        0xeeb70ac1
                    } else {
                        0xeeb70bc1
                    };
                    let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                    assert!(
                        matches!(
                            (&instruction, direction),
                            (Instruction::VCVT_f64_f32 { .. }, 0)
                                | (Instruction::VCVT_f32_f64 { .. }, 1)
                        ),
                        "{instruction:?}"
                    );
                    cpu.fp_regs.fill(0xdeadbeef);
                    cpu.fp_regs[2] = il;
                    cpu.fp_regs[3] = ih;
                    cpu.fpscr = control | sticky;
                    cpu.psr.value = 0xf90a0000;
                    let before = cpu.fp_regs;
                    if compiled {
                        cpu.execute_compiled(&instruction, 4);
                    } else {
                        cpu.execute(&instruction, 4);
                    }
                    assert_eq!(
                        (
                            cpu.fp_regs[0],
                            if direction == 0 { cpu.fp_regs[1] } else { 0 },
                            cpu.fpscr
                        ),
                        (lo, hi, status | sticky),
                        "{instruction:?} input={ih:08x}:{il:08x} control={control:08x} native={native} compiled={compiled}"
                    );
                    assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
                    assert_eq!(cpu.psr.value & 0xf80f0000, 0xf80a0000);
                    let at = if direction == 0 { 2 } else { 1 };
                    assert_eq!(cpu.fp_regs[at..], before[at..]);
                }
            }
        }
    }
}

#[test]
fn every_precision_source_and_destination_pair_preserves_other_registers() {
    let data = include_bytes!("../fixtures/isa_oracle_precision.bin");
    for direction in 0..2 {
        for single in 0..32 {
            for double in 0..16 {
                for native in [false, true] {
                    for compiled in [false, true] {
                        let index = ((single * 173 + double * 59) % 16_384) * 2 + direction;
                        let record = &data[index * 28..index * 28 + 28];
                        let [_, il, ih, control, lo, hi, status]: [u32; 7] =
                            core::array::from_fn(|i| {
                                u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
                            });
                        let opcode = if direction == 0 {
                            0xeeb70ac0
                                | ((double as u32) << 12)
                                | ((single as u32) >> 1)
                                | (((single as u32) & 1) << 5)
                        } else {
                            0xeeb70bc0
                                | (((single as u32) >> 1) << 12)
                                | (((single as u32) & 1) << 22)
                                | double as u32
                        };
                        let mut cpu = Processor::new();
                        cpu.cpacr = 0x00f00000;
                        cpu.native_float = native;
                        cpu.fp_regs.fill(0xdeadbeef);
                        if direction == 0 {
                            cpu.fp_regs[single] = il;
                        } else {
                            cpu.fp_regs[double * 2] = il;
                            cpu.fp_regs[double * 2 + 1] = ih;
                        }
                        let before = cpu.fp_regs;
                        cpu.fpscr = control;
                        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                        if compiled {
                            cpu.execute_compiled(&instruction, 4);
                        } else {
                            cpu.execute(&instruction, 4);
                        }
                        let start = if direction == 0 { double * 2 } else { single };
                        let width = if direction == 0 { 2 } else { 1 };
                        assert_eq!(
                            (
                                cpu.fp_regs[start],
                                if width == 2 {
                                    cpu.fp_regs[start + 1]
                                } else {
                                    0
                                },
                                cpu.fpscr
                            ),
                            (lo, hi, status),
                            "{instruction:?} source/destination overlap"
                        );
                        assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
                        for (i, prior) in before.into_iter().enumerate() {
                            if !(start..start + width).contains(&i) {
                                assert_eq!(cpu.fp_regs[i], prior, "{instruction:?} changed s{i}");
                            }
                        }
                    }
                }
            }
        }
    }
}
