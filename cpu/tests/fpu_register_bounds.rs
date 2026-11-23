#![cfg(feature = "has-fp")]
use zmu_cortex_m::{
    Processor,
    bus::Bus,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
    executor::Executor,
};

#[test]
fn single_stack_lists_transfer_exact_words_and_restore_the_stack() {
    let mut cpu = Processor::new();
    cpu.cpacr = 0x00f0_0000;
    for compiled in [false, true] {
        for first in 0..32u32 {
            for count in 1..=32 - first {
                let end = 0x2000_1000;
                let start = end - count * 4;
                cpu.msp = end;
                cpu.fpscr = 0xa000_009f;
                cpu.fp_regs = core::array::from_fn(|i| 0xff80_0001 ^ i as u32);
                let before = cpu.fp_regs;
                cpu.write32(start - 4, 0x1234_5678).unwrap();
                cpu.write32(end, 0x8765_4321).unwrap();
                let fields = ((first & 1) << 22) | ((first >> 1) << 12) | count;
                let push = cpu.decode(ThumbCode::Thumb32 {
                    opcode: 0xed2d0a00 | fields,
                });
                if compiled {
                    cpu.execute_compiled(&push, 4);
                } else {
                    cpu.execute(&push, 4);
                }
                assert_eq!(cpu.msp, start);
                for i in 0..count {
                    assert_eq!(
                        cpu.read32(start + i * 4).unwrap(),
                        before[(first + i) as usize]
                    );
                    cpu.fp_regs[(first + i) as usize] = 0;
                }
                let pop = cpu.decode(ThumbCode::Thumb32 {
                    opcode: 0xecbd0a00 | fields,
                });
                if compiled {
                    cpu.execute_compiled(&pop, 4);
                } else {
                    cpu.execute(&pop, 4);
                }
                assert_eq!(cpu.msp, end);
                assert_eq!(cpu.fp_regs, before);
                assert_eq!(cpu.read32(start - 4).unwrap(), 0x1234_5678);
                assert_eq!(cpu.read32(end).unwrap(), 0x8765_4321);
                assert_eq!(cpu.fpscr, 0xa000_009f, "bit transfers preserve FPSCR");
                assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
            }
        }
    }
}

#[test]
fn single_stack_lists_decode_every_populated_register() {
    let cpu = Processor::new();
    for first in 0..32u32 {
        for count in 1..=32 - first {
            let fields = ((first & 1) << 22) | ((first >> 1) << 12) | count;
            for base in [0xed2d0a00, 0xecbd0a00] {
                let opcode = base | fields;
                let decoded = cpu.decode(ThumbCode::Thumb32 { opcode });
                let params = match decoded {
                    Instruction::VPUSH { params } | Instruction::VPOP { params } => params,
                    _ => panic!("valid stack list {opcode:08x}: {decoded:?}"),
                };
                let registers: Vec<_> = params
                    .single_precision_registers
                    .iter()
                    .map(usize::from)
                    .collect();
                assert_eq!(
                    registers,
                    (first as usize..(first + count) as usize).collect::<Vec<_>>()
                );
                assert_eq!(params.imm32, 4 * count);
            }
        }
    }
}

#[test]
fn multiple_store_keeps_the_full_byte_count() {
    let cpu = Processor::new();
    // All 32 S registers or the same storage accessed as 16 D registers.
    for opcode in [0xeca00a20, 0xeca00b20] {
        match cpu.decode(ThumbCode::Thumb32 { opcode }) {
            Instruction::VSTM_T1 { params } => assert_eq!(params.imm32, 128),
            Instruction::VSTM_T2 { params } => assert_eq!(params.imm32, 128),
            other => panic!("{opcode:08x}: {other:?}"),
        }
    }
}

#[test]
fn malformed_single_lists_never_wrap_or_panic() {
    let cpu = Processor::new();
    for first in 0..32u32 {
        for count in 0..=255u32 {
            if count != 0 && first + count <= 32 {
                continue;
            }
            let fields = ((first & 1) << 22) | ((first >> 1) << 12) | count;
            for base in [0xed2d0a00, 0xecbd0a00, 0xeca00a00, 0xecb00a00] {
                let opcode = base | fields;
                let decoded = cpu.decode(ThumbCode::Thumb32 { opcode });
                assert!(
                    matches!(decoded, Instruction::UDF { .. }),
                    "{opcode:08x}: {decoded:?}"
                );
            }
        }
    }
}

#[cfg(feature = "vfp-register-bank-d16")]
#[test]
fn double_arithmetic_and_transfers_reject_unpopulated_registers() {
    let cpu = Processor::new();
    // Independent literal instruction families; each valid baseline uses D0–D2.
    for (opcode, register_bits) in [
        (0xee310b02, 0x0040_00a0), // VADD
        (0xee310b42, 0x0040_00a0), // VSUB
        (0xee210b02, 0x0040_00a0), // VMUL
        (0xee810b02, 0x0040_00a0), // VDIV
        (0xeea10b02, 0x0040_00a0), // VFMA
        (0xeeb00b41, 0x0040_0020), // VMOV
        (0xeeb00bc1, 0x0040_0020), // VABS
        (0xeeb10b41, 0x0040_0020), // VNEG
        (0xeeb10bc1, 0x0040_0020), // VSQRT
        (0xeeb40b41, 0x0040_0020), // VCMP
        (0xeeb70bc1, 0x0000_0020), // VCVT f32 <- f64
        (0xeeb70ac0, 0x0040_0000), // VCVT f64 <- f32
        (0xed900b00, 0x0040_0000), // VLDR
        (0xed800b00, 0x0040_0000), // VSTR
        (0xeeb70b00, 0x0040_0000), // VMOV immediate
        (0xec410b10, 0x0000_0020), // VMOV core pair -> double
    ] {
        if cfg!(feature = "fpv5-d16") {
            assert!(
                !matches!(
                    cpu.decode(ThumbCode::Thumb32 { opcode }),
                    Instruction::UDF { .. }
                ),
                "valid double baseline {opcode:08x}"
            );
        }
        for bit in [1 << 22, 1 << 7, 1 << 5] {
            if register_bits & bit == 0 {
                continue;
            }
            let invalid = opcode | bit;
            let decoded = cpu.decode(ThumbCode::Thumb32 { opcode: invalid });
            assert!(
                matches!(decoded, Instruction::UDF { .. }),
                "unpopulated {invalid:08x}: {decoded:?}"
            );
        }
    }
}

#[cfg(not(feature = "fpv5-d16"))]
#[test]
fn single_only_profiles_keep_double_storage_but_reject_double_arithmetic() {
    let cpu = Processor::new();
    for opcode in [
        0xeeb00b41, 0xec410b10, 0xed900b00, 0xed800b00, 0xed2d8b10, 0xecbd8b10,
    ] {
        assert!(
            !matches!(
                cpu.decode(ThumbCode::Thumb32 { opcode }),
                Instruction::UDF { .. }
            ),
            "valid doubleword storage {opcode:08x}"
        );
    }
    for opcode in [
        0xee310b02, 0xee310b42, 0xee210b02, 0xee810b02, 0xeea10b02, 0xeeb00bc1, 0xeeb10b41,
        0xeeb10bc1, 0xeeb40b41, 0xeeb70bc1, 0xeeb70ac0, 0xeeb70b00,
    ] {
        assert!(
            matches!(
                cpu.decode(ThumbCode::Thumb32 { opcode }),
                Instruction::UDF { .. }
            ),
            "unsupported double arithmetic {opcode:08x}"
        );
    }
}

#[test]
fn single_arithmetic_keeps_all_thirty_two_registers() {
    let cpu = Processor::new();
    for opcode in [
        0xee310a02, 0xee310a42, 0xee210a02, 0xee810a02, 0xeea10a02, 0xeeb00a41, 0xeeb00ac1,
        0xeeb10a41, 0xeeb10ac1, 0xeeb40a41, 0xed900a00, 0xed800a00, 0xeeb70a00,
    ] {
        for fields in [0, 1 << 22, 1 << 5, 0x0040_0020] {
            if opcode == 0xeeb70a00 && fields & (1 << 5) != 0 {
                continue;
            }
            let decoded = cpu.decode(ThumbCode::Thumb32 {
                opcode: opcode | fields,
            });
            assert!(
                !matches!(decoded, Instruction::UDF { .. }),
                "valid single {opcode:08x} + {fields:x}"
            );
        }
    }
}

#[cfg(feature = "vfp-register-bank-d16")]
#[test]
fn double_multiple_lists_stop_at_d15() {
    let cpu = Processor::new();
    for first in 0..32u32 {
        for count in 0..=127u32 {
            for base in [0xed2d0b00, 0xecbd0b00, 0xeca00b00, 0xecb00b00] {
                let opcode = base | ((first >> 4) << 22) | ((first & 15) << 12) | (count * 2);
                let decoded = cpu.decode(ThumbCode::Thumb32 { opcode });
                assert_eq!(
                    matches!(decoded, Instruction::UDF { .. }),
                    count == 0 || first + count > 16,
                    "{opcode:08x}: {decoded:?}"
                );
            }
        }
    }
}
