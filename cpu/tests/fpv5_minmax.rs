#![cfg(any(feature = "fpv5-d16", feature = "fpv5-sp-d16"))]
use zmu_cortex_m::{Processor, core::thumb::ThumbCode, decoder::Decoder, executor::Executor};

#[test]
fn coprocessor_transfer_masks_do_not_capture_data_processing_encodings() {
    use zmu_cortex_m::core::instruction::Instruction;
    let cpu = Processor::new();
    assert!(matches!(
        cpu.decode(ThumbCode::Thumb32 { opcode: 0xee000c10 }),
        Instruction::MCR { coproc: 12, .. }
    ));
    assert!(matches!(
        cpu.decode(ThumbCode::Thumb32 { opcode: 0xfe000c10 }),
        Instruction::MCR2 { coproc: 12, .. }
    ));
    for opcode in [0xee800c01, 0xfe800c01] {
        assert!(matches!(
            cpu.decode(ThumbCode::Thumb32 { opcode }),
            Instruction::UDF { .. }
        ));
    }
}

#[test]
fn llvm_unsigned_cast_clamp_keeps_the_fractional_pll_numerator() {
    let mut cpu = Processor::new();
    cpu.cpacr = 0x00f00000;
    cpu.fp_regs[0] = 827.0f32.to_bits();
    cpu.fp_regs[2] = 0;
    cpu.fp_regs[4] = 65535.0f32.to_bits();
    for opcode in [0xfe801a01, 0xfe811a42] {
        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
        cpu.execute(&instruction, 4);
        assert_eq!(cpu.fp_regs[2], 827.0f32.to_bits(), "{instruction:?}");
    }
    for opcode in [0xeebc1ac1, 0xee110a10] {
        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
        cpu.execute(&instruction, 4);
    }
    assert_eq!(cpu.r0_12[0], 827, "complete LLVM u16 cast");
}

#[test]
fn minmax_results_and_status_match_the_executed_qemu_fpv5_oracle() {
    for record in include_bytes!("../fixtures/isa_oracle_minmax.bin")
        .as_chunks::<40>()
        .0
    {
        let words: Vec<u32> = record
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        let [precision, al, ah, bl, bh, minimum, control, lo, hi, status] = words[..] else {
            unreachable!()
        };
        if precision == 64 && !cfg!(feature = "fpv5-d16") {
            continue;
        }
        for native in [false, true] {
            for compiled in [false, true] {
                let mut cpu = Processor::new();
                cpu.cpacr = 0x00f00000;
                cpu.native_float = native;
                cpu.fpscr = control;
                cpu.psr.value = 0xf1000000;
                let opcode = if precision == 32 {
                    cpu.fp_regs[1] = al;
                    cpu.fp_regs[2] = bl;
                    0xfe800a81
                } else {
                    cpu.fp_regs[2] = al;
                    cpu.fp_regs[3] = ah;
                    cpu.fp_regs[4] = bl;
                    cpu.fp_regs[5] = bh;
                    0xfe810b02
                } | (minimum << 6);
                let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                if compiled {
                    cpu.execute_compiled(&instruction, 4);
                } else {
                    cpu.execute(&instruction, 4);
                }
                let high = if precision == 64 { cpu.fp_regs[1] } else { 0 };
                assert_eq!(
                    (cpu.fp_regs[0], high, cpu.fpscr),
                    (lo, hi, status),
                    "{precision} {al:08x}/{ah:08x} {bl:08x}/{bh:08x} min={minimum} control={control:08x} native={native} compiled={compiled}"
                );
                assert_eq!(cpu.psr.value & 0xf0000000, 0xf0000000, "APSR flags changed");
            }
        }
    }
}

#[test]
fn high_registers_and_source_destination_aliases_are_decoded_correctly() {
    for (d, n, m) in [(31u32, 30u32, 29u32), (0, 0, 1), (16, 15, 16), (1, 0, 1)] {
        for minimum in [false, true] {
            let mut cpu = Processor::new();
            cpu.cpacr = 0x00f00000;
            cpu.fp_regs[n as usize] = 3.5f32.to_bits();
            cpu.fp_regs[m as usize] = (-9.0f32).to_bits();
            let opcode = 0xfe800a00
                | ((d & 1) << 22)
                | ((d >> 1) << 12)
                | ((n & 1) << 7)
                | ((n >> 1) << 16)
                | ((m & 1) << 5)
                | (m >> 1)
                | (u32::from(minimum) << 6);
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            cpu.execute(&instruction, 4);
            assert_eq!(
                cpu.fp_regs[d as usize],
                if minimum {
                    (-9.0f32).to_bits()
                } else {
                    3.5f32.to_bits()
                }
            );
        }
    }
}

#[cfg(feature = "fpv5-d16")]
#[test]
fn double_register_bank_boundaries_are_checked() {
    use zmu_cortex_m::core::instruction::Instruction;
    for (d, n, m) in [(15u32, 14u32, 13u32), (0, 0, 1), (8, 7, 8)] {
        for minimum in [false, true] {
            let mut cpu = Processor::new();
            cpu.cpacr = 0x00f00000;
            for (reg, value) in [(n, 3.5f64), (m, -9.0f64)] {
                let bits = value.to_bits();
                cpu.fp_regs[(reg * 2) as usize] = bits as u32;
                cpu.fp_regs[(reg * 2 + 1) as usize] = (bits >> 32) as u32;
            }
            let opcode = 0xfe800b00 | (d << 12) | (n << 16) | m | (u32::from(minimum) << 6);
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            cpu.execute(&instruction, 4);
            let bits = u64::from(cpu.fp_regs[(d * 2) as usize])
                | (u64::from(cpu.fp_regs[(d * 2 + 1) as usize]) << 32);
            assert_eq!(
                bits,
                if minimum {
                    (-9.0f64).to_bits()
                } else {
                    3.5f64.to_bits()
                }
            );
        }
    }
    let cpu = Processor::new();
    for opcode in [0xfec00b02, 0xfe800b82, 0xfe800b22] {
        assert!(matches!(
            cpu.decode(ThumbCode::Thumb32 { opcode }),
            Instruction::UDF { .. }
        ));
    }
}
