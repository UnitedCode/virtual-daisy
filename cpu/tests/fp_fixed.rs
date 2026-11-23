#![cfg(feature = "has-fp")]
use zmu_cortex_m::{
    Processor,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
    executor::Executor,
};
#[path = "../fixtures/fixed_probe_cases.rs"]
mod fixed_cases;

#[test]
fn standalone_guest_cases_are_pinned_to_independently_executed_records() {
    let data = include_bytes!("../fixtures/isa_oracle_fixed.bin");
    for expected in fixed_cases::CASES {
        assert!(
            data.as_chunks::<32>().0.iter().any(|record| {
                let row: [u32; 7] = core::array::from_fn(|i| {
                    u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
                });
                row == expected
            }),
            "guest case absent from ARM fixture: {expected:x?}"
        );
    }
}

fn check(double: bool, to_fixed: bool) {
    if double && !cfg!(feature = "fpv5-d16") {
        return;
    }
    let data = include_bytes!("../fixtures/isa_oracle_fixed.bin");
    assert_eq!(data.len(), 401_408 * 32);
    for native in [false, true] {
        for compiled in [false, true] {
            for sticky in [0, 0x9f] {
                let mut cpu = Processor::new();
                cpu.cpacr = 0x00f00000;
                cpu.native_float = native;
                for (index, record) in data.as_chunks::<32>().0.iter().enumerate() {
                    let [opcode, il, ih, control, lo, hi, status, magic]: [u32; 8] =
                        core::array::from_fn(|i| {
                            u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
                        });
                    assert_eq!(magic, 0x44584946);
                    if (opcode & (1 << 8) != 0) != double || (opcode & (1 << 18) != 0) != to_fixed {
                        continue;
                    }
                    let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                    assert!(
                        !matches!(instruction, Instruction::UDF { .. }),
                        "missing {opcode:08x}"
                    );
                    cpu.fp_regs.fill(0xdeadbeef);
                    cpu.fp_regs[0] = il;
                    cpu.fp_regs[1] = ih;
                    cpu.fpscr = control | sticky;
                    cpu.psr.value = 0xf90a0000;
                    let before = cpu.fp_regs;
                    if compiled {
                        cpu.execute_compiled(&instruction, 4);
                    } else {
                        cpu.execute(&instruction, 4);
                    }
                    assert_eq!(
                        (cpu.fp_regs[0], cpu.fp_regs[1], cpu.fpscr),
                        (lo, hi, status | sticky),
                        "record={index} {instruction:?} opcode={opcode:08x} input={ih:08x}:{il:08x} control={control:08x} native={native} compiled={compiled}"
                    );
                    assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
                    assert_eq!(cpu.psr.value & 0xf80f0000, 0xf80a0000);
                    assert_eq!(cpu.fp_regs[2..], before[2..]);
                }
            }
        }
    }
}
#[test]
fn single_to_fixed() {
    check(false, true);
}
#[test]
fn single_from_fixed() {
    check(false, false);
}
#[test]
fn double_to_fixed() {
    check(true, true);
}
#[test]
fn double_from_fixed() {
    check(true, false);
}

#[test]
fn every_fixed_conversion_register_preserves_its_neighbors() {
    let data = include_bytes!("../fixtures/isa_oracle_fixed.bin");
    for block in data.chunks_exact(1024 * 32) {
        for sample in [5usize, 8, 12] {
            let record = &block[sample * 16 * 32..(sample * 16 + 1) * 32];
            let [base, il, ih, control, lo, hi, status, _]: [u32; 8] = core::array::from_fn(|i| {
                u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
            });
            let double = base & (1 << 8) != 0;
            if double && !cfg!(feature = "fpv5-d16") {
                continue;
            }
            for register in 0..if double { 16 } else { 32 } {
                let opcode = (base & !0x0040f000)
                    | if double {
                        register << 12
                    } else {
                        (register >> 1) << 12 | (register & 1) << 22
                    };
                for compiled in [false, true] {
                    let mut cpu = Processor::new();
                    cpu.cpacr = 0x00f00000;
                    cpu.fp_regs = core::array::from_fn(|i| 0xdeadbeef ^ (i as u32 * 0x01010101));
                    let start = if double { register * 2 } else { register } as usize;
                    cpu.fp_regs[start] = il;
                    if double {
                        cpu.fp_regs[start + 1] = ih;
                    }
                    cpu.fpscr = control;
                    let before = cpu.fp_regs;
                    let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
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
                        (cpu.fp_regs[start], cpu.fpscr),
                        (lo, status),
                        "{opcode:08x}"
                    );
                    if double {
                        assert_eq!(cpu.fp_regs[start + 1], hi, "{opcode:08x}");
                    }
                    let written = start..start + if double { 2 } else { 1 };
                    for (i, value) in before.into_iter().enumerate() {
                        if !written.contains(&i) {
                            assert_eq!(cpu.fp_regs[i], value, "{opcode:08x} changed s{i}");
                        }
                    }
                    assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
                }
            }
        }
    }
}

#[test]
fn invalid_fraction_and_double_bank_encodings_are_undefined() {
    let cpu = Processor::new();
    for double in [false, true] {
        for to_fixed in [false, true] {
            for unsigned in [false, true] {
                let base = 0xeeba0a40
                    | u32::from(double) << 8
                    | u32::from(to_fixed) << 18
                    | u32::from(unsigned) << 16;
                for immediate in 17..32 {
                    let opcode = base | (immediate >> 1) | (immediate & 1) << 5;
                    assert!(matches!(
                        cpu.decode(ThumbCode::Thumb32 { opcode }),
                        Instruction::UDF { .. }
                    ));
                }
                if double {
                    for register in 16..32 {
                        let opcode = base | (register & 15) << 12 | 1 << 22;
                        assert!(matches!(
                            cpu.decode(ThumbCode::Thumb32 { opcode }),
                            Instruction::UDF { .. }
                        ));
                    }
                    assert_eq!(
                        !matches!(
                            cpu.decode(ThumbCode::Thumb32 { opcode: base }),
                            Instruction::UDF { .. }
                        ),
                        cfg!(feature = "fpv5-d16")
                    );
                }
            }
        }
    }
}

#[test]
fn fixed_conversions_obey_it_and_check_coprocessor_before_writing() {
    for opcode in [
        0xeeba0a40, 0xeebe0a40, 0xeebb0ac0, 0xeebf0ac0, 0xeeba0b40, 0xeebe0b40, 0xeebb0bc0,
        0xeebf0bc0,
    ] {
        if opcode & (1 << 8) != 0 && !cfg!(feature = "fpv5-d16") {
            continue;
        }
        for compiled in [false, true] {
            for skipped in [false, true] {
                let mut cpu = Processor::new();
                cpu.fp_regs.fill(0xa5a55a5a);
                cpu.fpscr = 0x0700009f;
                if skipped {
                    cpu.psr.value = 1 << 30;
                    let it = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf18 });
                    cpu.execute(&it, 2);
                }
                let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                if compiled {
                    cpu.execute_compiled(&instruction, 4);
                } else {
                    cpu.execute(&instruction, 4);
                }
                assert_eq!(cpu.fp_regs, [0xa5a55a5a; 32]);
                assert_eq!(cpu.fpscr, 0x0700009f);
                if skipped {
                    assert_eq!((cpu.cfsr, cpu.hfsr, cpu.get_itstate()), (0, 0, 0));
                } else {
                    assert_ne!(cpu.cfsr & (1 << 19), 0);
                }
            }
        }
    }
}
