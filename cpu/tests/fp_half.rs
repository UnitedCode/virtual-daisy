#![cfg(feature = "has-fp")]
use zmu_cortex_m::{
    Processor,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
    executor::Executor,
};
#[path = "../fixtures/half_probe.rs"]
mod half_probe;

#[test]
fn recorded_legacy_reference_fault_is_rejected_by_the_positive_gate() {
    let capture = include_bytes!("../fixtures/half_legacy_reference.bin");
    assert_eq!(capture.len(), 132);
    half_probe::check_legacy_double(&capture[..128], &capture[128..]).unwrap();
    assert!(half_probe::check(&capture[..128]).is_err());
}

#[test]
fn shared_reference_conversion_api_matches_every_arm_record() {
    let data = include_bytes!("../fixtures/isa_oracle_half.bin");
    for record in data.as_chunks::<36>().0 {
        let [op, il, ih, _, control, lo, hi, status, _]: [u32; 9] = core::array::from_fn(|i| {
            u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
        });
        let to_half = op % 4 >= 2;
        let precision = if op >= 4 { 64 } else { 32 };
        let shift = 16 * (op & 1);
        let bits = if to_half {
            u64::from(il) | u64::from(ih) << 32
        } else {
            u64::from((il >> shift) as u16)
        };
        let expected = if to_half {
            u64::from((lo >> shift) as u16)
        } else {
            u64::from(lo) | u64::from(hi) << 32
        };
        assert_eq!(
            zmu_cortex_m::convert_half_bits(bits, precision, to_half, control),
            Some((expected, status & 0x9f))
        );
    }
    assert!(zmu_cortex_m::convert_half_bits(0, 16, false, 0).is_none());
    assert!(zmu_cortex_m::convert_half_bits(0x10000, 32, false, 0).is_none());
    assert!(zmu_cortex_m::convert_half_bits(1u64 << 32, 32, true, 0).is_none());
}

#[test]
fn standalone_guest_expectations_are_in_the_independent_arm_fixture() {
    let data = include_bytes!("../fixtures/isa_oracle_half.bin");
    let mut capture = [0u32; 32];
    capture[0] = 0xab00f016;
    for (index, (operation, input, control, expected)) in half_probe::CASES.into_iter().enumerate()
    {
        let actual = data
            .as_chunks::<36>()
            .0
            .iter()
            .find_map(|record| {
                let words: [u32; 9] = core::array::from_fn(|i| {
                    u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
                });
                if words[0] != operation || words[4] != control {
                    return None;
                }
                let source = u64::from(words[1]) | u64::from(words[2]) << 32;
                let same = if operation % 4 < 2 {
                    let shift = 16 * (operation & 1);
                    ((source >> shift) as u16) == ((input >> shift) as u16)
                } else {
                    source == input
                };
                same.then_some([words[5], words[6], words[7]])
            })
            .expect("standalone guest case absent from ARM fixture");
        assert_eq!(actual, expected);
        capture[1 + 3 * index..4 + 3 * index].copy_from_slice(&actual);
    }
    let (count, hash) = half_probe::expected_register_matrix();
    capture[27] = count;
    capture[28] = hash as u32;
    capture[29] = (hash >> 32) as u32;
    capture[30] = 1;
    half_probe::check(
        &capture
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
}

fn check(operation: u32) {
    if operation >= 4 && !cfg!(feature = "fpv5-d16") {
        return;
    }
    let data = include_bytes!("../fixtures/isa_oracle_half.bin");
    assert_eq!(data.len(), 331_776 * 36);
    let opcode = [
        0xeeb20a41, 0xeeb20ac1, 0xeeb30a41, 0xeeb30ac1, 0xeeb20b41, 0xeeb20bc1, 0xeeb30b41,
        0xeeb30bc1,
    ][operation as usize];
    for native in [false, true] {
        for compiled in [false, true] {
            for sticky in [0, 0x9f] {
                let mut cpu = Processor::new();
                cpu.cpacr = 0x00f00000;
                cpu.native_float = native;
                let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                assert!(
                    !matches!(instruction, Instruction::UDF { .. }),
                    "missing {opcode:08x}"
                );
                for record in data.as_chunks::<36>().0 {
                    let [op, il, ih, canary, control, lo, hi, status, magic]: [u32; 9] =
                        core::array::from_fn(|i| {
                            u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
                        });
                    assert_eq!(magic, 0x464c4148);
                    if op != operation {
                        continue;
                    }
                    cpu.fp_regs.fill(0xdeadbeef);
                    cpu.fp_regs[0] = canary;
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
                    let wide = operation == 4 || operation == 5;
                    assert_eq!(
                        (
                            cpu.fp_regs[0],
                            if wide { cpu.fp_regs[1] } else { 0 },
                            cpu.fpscr
                        ),
                        (lo, hi, status | sticky),
                        "{instruction:?} input={ih:08x}:{il:08x} control={control:08x} native={native} compiled={compiled}"
                    );
                    assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
                    assert_eq!(cpu.psr.value & 0xf80f0000, 0xf80a0000);
                    let at = if wide { 2 } else { 1 };
                    assert_eq!(cpu.fp_regs[at..], before[at..]);
                }
            }
        }
    }
}
macro_rules! cases {($(($name:ident,$op:expr)),*$(,)?)=>{$(#[test]fn $name(){check($op);})*};}
cases! {(from_half_single_bottom,0),(from_half_single_top,1),(to_half_single_bottom,2),(to_half_single_top,3),
(from_half_double_bottom,4),(from_half_double_top,5),(to_half_double_bottom,6),(to_half_double_top,7)}

#[test]
fn every_half_and_full_register_pair_preserves_unwritten_bits() {
    let data = include_bytes!("../fixtures/isa_oracle_half.bin");
    let bases = [
        0xeeb20a41, 0xeeb20ac1, 0xeeb30a41, 0xeeb30ac1, 0xeeb20b41, 0xeeb20bc1, 0xeeb30b41,
        0xeeb30bc1,
    ];
    for (operation, base) in bases.into_iter().enumerate() {
        let double = operation >= 4;
        if double && !cfg!(feature = "fpv5-d16") {
            continue;
        }
        let to_half = operation % 4 >= 2;
        let samples: Vec<[u32; 9]> = data
            .as_chunks::<36>()
            .0
            .iter()
            .map(|record| {
                core::array::from_fn(|i| {
                    u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
                })
            })
            .filter(|words| words[0] as usize == operation)
            .step_by(131)
            .collect();
        for half in 0..32 {
            for full in 0..if double { 16 } else { 32 } {
                for native in [false, true] {
                    for compiled in [false, true] {
                        let [_, il, ih, canary, control, lo, hi, status, _] =
                            samples[(half * 71 + full * 13) % samples.len()];
                        let encode = |register: usize, wide: bool| {
                            if wide {
                                (register as u32, 0)
                            } else {
                                ((register >> 1) as u32, (register & 1) as u32)
                            }
                        };
                        let (vd, d) = if to_half {
                            encode(half, false)
                        } else {
                            encode(full, double)
                        };
                        let (vm, m) = if to_half {
                            encode(full, double)
                        } else {
                            encode(half, false)
                        };
                        let opcode = (base & !0x0040f02f) | (vd << 12) | (d << 22) | vm | (m << 5);
                        let mut cpu = Processor::new();
                        cpu.cpacr = 0x00f00000;
                        cpu.native_float = native;
                        cpu.fp_regs.fill(0xdeadbeef);
                        if to_half {
                            cpu.fp_regs[half] = canary;
                            if double {
                                cpu.fp_regs[2 * full] = il;
                                cpu.fp_regs[2 * full + 1] = ih;
                            } else {
                                cpu.fp_regs[full] = il;
                            }
                        } else {
                            cpu.fp_regs[half] = il;
                        }
                        let before = cpu.fp_regs;
                        cpu.fpscr = control;
                        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                        if compiled {
                            cpu.execute_compiled(&instruction, 4);
                        } else {
                            cpu.execute(&instruction, 4);
                        }
                        let start = if to_half {
                            half
                        } else if double {
                            2 * full
                        } else {
                            full
                        };
                        let width = if double && !to_half { 2 } else { 1 };
                        let expected = if to_half {
                            let mask = 0xffffu32 << if operation & 1 == 0 { 0 } else { 16 };
                            (lo & mask) | (before[half] & !mask)
                        } else {
                            lo
                        };
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
                            (expected, hi, status),
                            "{instruction:?} overlapping registers"
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

#[test]
fn full_precision_bank_bits_do_not_discard_half_register_bits() {
    let cpu = Processor::new();
    for base in [0xeeb20b41, 0xeeb20bc1, 0xeeb30b41, 0xeeb30bc1] {
        let to_half = base & (1 << 16) != 0;
        let unavailable = if to_half { 1 << 5 } else { 1 << 22 };
        let half_bit = if to_half { 1 << 22 } else { 1 << 5 };
        for half in [0, half_bit] {
            assert!(matches!(
                cpu.decode(ThumbCode::Thumb32 {
                    opcode: base | unavailable | half
                }),
                Instruction::UDF { .. }
            ));
            assert_eq!(
                !matches!(
                    cpu.decode(ThumbCode::Thumb32 {
                        opcode: base | half
                    }),
                    Instruction::UDF { .. }
                ),
                cfg!(feature = "fpv5-d16")
            );
        }
    }
}

#[test]
fn skipped_half_conversions_do_not_touch_registers_or_raise_nocp() {
    for opcode in [
        0xeeb20a41, 0xeeb20ac1, 0xeeb30a41, 0xeeb30ac1, 0xeeb20b41, 0xeeb20bc1, 0xeeb30b41,
        0xeeb30bc1,
    ] {
        if opcode & (1 << 8) != 0 && !cfg!(feature = "fpv5-d16") {
            continue;
        }
        for compiled in [false, true] {
            let mut cpu = Processor::new();
            cpu.psr.value = 1 << 30;
            cpu.fp_regs.fill(0xa5a55a5a);
            cpu.fpscr = 0x0700009f;
            let it = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf18 });
            cpu.execute(&it, 2);
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_eq!(cpu.fp_regs, [0xa5a55a5a; 32]);
            assert_eq!(cpu.fpscr, 0x0700009f);
            assert_eq!((cpu.cfsr, cpu.hfsr, cpu.get_itstate()), (0, 0, 0));
        }
    }
}

#[test]
fn enabled_half_opcode_with_disabled_coprocessor_raises_nocp_before_writing() {
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        cpu.fp_regs.fill(0xa5a55a5a);
        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode: 0xeeb20a41 });
        if compiled {
            cpu.execute_compiled(&instruction, 4);
        } else {
            cpu.execute(&instruction, 4);
        }
        assert_ne!(cpu.cfsr & (1 << 19), 0);
        assert_eq!(cpu.fp_regs, [0xa5a55a5a; 32]);
    }
}
