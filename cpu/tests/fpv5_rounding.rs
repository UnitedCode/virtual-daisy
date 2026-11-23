#![cfg(any(feature = "fpv5-d16", feature = "fpv5-sp-d16"))]
use zmu_cortex_m::{
    Processor,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
    executor::Executor,
};

// Literal encodings checked against the independently assembled QEMU guest.
const SINGLE: [u32; 19] = [
    0xfeb80a60, 0xfeb90a60, 0xfeba0a60, 0xfebb0a60, 0xeeb60ae0, 0xeeb60a60, 0xeeb70a60, 0xfebc0ae0,
    0xfebc0a60, 0xfebd0ae0, 0xfebd0a60, 0xfebe0ae0, 0xfebe0a60, 0xfebf0ae0, 0xfebf0a60, 0xeebd0ae0,
    0xeebc0ae0, 0xeebd0a60, 0xeebc0a60,
];
const DOUBLE: [u32; 19] = [
    0xfeb80b41, 0xfeb90b41, 0xfeba0b41, 0xfebb0b41, 0xeeb60bc1, 0xeeb60b41, 0xeeb70b41, 0xfebc0bc1,
    0xfebc0b41, 0xfebd0bc1, 0xfebd0b41, 0xfebe0bc1, 0xfebe0b41, 0xfebf0bc1, 0xfebf0b41, 0xeebd0bc1,
    0xeebc0bc1, 0xeebd0b41, 0xeebc0b41,
];
fn cases() -> impl Iterator<Item = [u32; 9]> {
    let data = include_bytes!("../fixtures/isa_oracle_rounding.bin");
    assert_eq!(data.len() % 36, 0);
    data.as_chunks::<36>().0.iter().map(|bytes| {
        core::array::from_fn(|i| u32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap()))
    })
}
fn check(record: [u32; 9]) {
    check_registers(record, 0, 1);
}
fn check_registers(record: [u32; 9], destination: usize, source: usize) {
    let [
        magic,
        precision,
        lo,
        hi,
        operation,
        control,
        expected_lo,
        expected_hi,
        status,
    ] = record;
    assert_eq!(magic, 0x544e_5256);
    if precision == 64 && !cfg!(feature = "fpv5-d16") {
        return;
    }
    let double_result = precision == 64 && operation < 7;
    let mut opcode = if precision == 32 { SINGLE } else { DOUBLE }[operation as usize];
    opcode &= !0x0040_f02f;
    opcode |= if double_result {
        (destination as u32) << 12
    } else {
        (((destination as u32) & 1) << 22) | (((destination as u32) >> 1) << 12)
    };
    opcode |= if precision == 32 {
        (((source as u32) & 1) << 5) | ((source as u32) >> 1)
    } else {
        source as u32
    };
    for native in [false, true] {
        for compiled in [false, true] {
            let mut cpu = Processor::new();
            cpu.cpacr = 0x00f0_0000;
            cpu.native_float = native;
            cpu.psr.value = 0xf90a_0000;
            cpu.fpscr = control;
            for (index, value) in cpu.fp_regs.iter_mut().enumerate() {
                *value = 0xdead_beef ^ index as u32;
            }
            if precision == 32 {
                cpu.fp_regs[source] = lo;
            } else {
                cpu.fp_regs[2 * source] = lo;
                cpu.fp_regs[2 * source + 1] = hi;
            }
            let prior = cpu.fp_regs;
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            assert!(
                !matches!(instruction, Instruction::UDF { .. }),
                "missing {opcode:08x}"
            );
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            let count = if double_result { 2 } else { 1 };
            let start = if double_result {
                2 * destination
            } else {
                destination
            };
            let result_hi = if count == 2 {
                cpu.fp_regs[start + 1]
            } else {
                0
            };
            assert_eq!(
                (cpu.fp_regs[start], result_hi, cpu.fpscr),
                (expected_lo, expected_hi, status),
                "{instruction:?}: op={operation} input={hi:08x}:{lo:08x} control={control:08x} native={native} compiled={compiled}"
            );
            assert_eq!(cpu.cfsr, 0, "{instruction:?}");
            assert_eq!(cpu.hfsr, 0, "{instruction:?}");
            assert_eq!(cpu.psr.value & 0xf80f_0000, 0xf80a_0000, "APSR changed");
            for (index, &value) in prior.iter().enumerate() {
                if !(start..start + count).contains(&index) {
                    assert_eq!(
                        cpu.fp_regs[index], value,
                        "non-destination FP register {index} changed"
                    );
                }
            }
        }
    }
}
#[test]
fn rounding_register_fields_cover_high_registers_and_aliases() {
    for precision in [32, 64] {
        if precision == 64 && !cfg!(feature = "fpv5-d16") {
            continue;
        }
        for operation in 0..19 {
            let record = cases()
                .find(|r| {
                    r[1] == precision
                        && r[4] == operation
                        && r[5] == 0xa000_0000
                        && if precision == 32 {
                            r[2] == 1.5f32.to_bits()
                        } else {
                            u64::from(r[2]) | (u64::from(r[3]) << 32) == 1.5f64.to_bits()
                        }
                })
                .unwrap();
            let destinations = if precision == 64 && operation < 7 {
                16
            } else {
                32
            };
            let sources = if precision == 64 { 16 } else { 32 };
            for destination in 0..destinations {
                for source in 0..sources {
                    check_registers(record, destination, source);
                }
            }
        }
    }
}
#[test]
fn directed_rounding_and_conversions_match_executed_qemu_results() {
    for record in cases().filter(|r| r[4] < 15) {
        check(record);
    }
}
#[test]
fn existing_round_to_zero_preserves_large_integral_operands() {
    for record in cases().filter(|r| r[4] == 4) {
        let large = if record[1] == 32 {
            record[2] & 0x7fff_ffff >= 0x4b00_0000
        } else {
            record[3] & 0x7fff_ffff >= 0x4330_0000
        };
        if large {
            check(record);
        }
    }
}
#[test]
fn existing_float_to_integer_conversions_match_qemu_rounding_and_flags() {
    for record in cases().filter(|r| r[4] >= 15) {
        check(record);
    }
}
#[test]
fn double_rounding_and_conversion_encodings_reject_the_unpopulated_bank() {
    let cpu = Processor::new();
    for (operation, &opcode) in DOUBLE.iter().enumerate() {
        let source_d16 = (opcode & !0x2f) | 0x20;
        assert!(
            matches!(
                cpu.decode(ThumbCode::Thumb32 { opcode: source_d16 }),
                Instruction::UDF { .. }
            ),
            "D16 source {source_d16:08x}"
        );
        if operation < 7 {
            let destination_d16 = opcode | (1 << 22);
            assert!(
                matches!(
                    cpu.decode(ThumbCode::Thumb32 {
                        opcode: destination_d16
                    }),
                    Instruction::UDF { .. }
                ),
                "D16 destination {destination_d16:08x}"
            );
        }
    }
}
