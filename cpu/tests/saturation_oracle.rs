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
const FLAGS: u32 = 0xf80f0000;
#[path = "../fixtures/saturation_probe_cases.rs"]
mod probe_cases;
#[test]
fn arm_guest_cases_are_present_in_the_independent_oracle() {
    let data = include_bytes!("../fixtures/isa_oracle_saturate.bin");
    for expected in probe_cases::CASES {
        assert!(
            data.as_chunks::<24>().0.iter().any(|record| {
                let row: [u32; 5] = core::array::from_fn(|i| {
                    u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
                });
                row == expected
            }),
            "ARM guest case absent from QEMU fixture: {expected:x?}"
        );
    }
}
fn lanes(opcode: u32) -> bool {
    opcode & 0x002070c0 == 0x00200000
}
fn check(unsigned: bool, halfwords: bool) {
    let data = include_bytes!("../fixtures/isa_oracle_saturate.bin");
    assert_eq!(data.len(), 520_192 * 24);
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for (index, record) in data.as_chunks::<24>().0.iter().enumerate() {
            let [opcode, input, control, result, status, magic]: [u32; 6] =
                core::array::from_fn(|i| {
                    u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
                });
            assert_eq!(magic, 0x54415353);
            if (opcode & (1 << 23) != 0) != unsigned || lanes(opcode) != halfwords {
                continue;
            }
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            if halfwords && !cfg!(feature = "has-dsp-ext") {
                assert!(matches!(instruction, Instruction::UDF { .. }));
                continue;
            }
            assert!(
                !matches!(instruction, Instruction::UDF { .. }),
                "missing {opcode:08x}"
            );
            cpu.set_r(Reg::R0, 0xdeadbeef);
            cpu.set_r(Reg::R1, input);
            cpu.psr.value = control | (1 << 24);
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_eq!(
                (cpu.get_r(Reg::R0), cpu.psr.value & FLAGS),
                (result, status & FLAGS),
                "case={index} {instruction:?} input={input:08x} control={control:08x}"
            );
            assert_eq!(cpu.get_r(Reg::R1), input);
            assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
        }
    }
}
#[test]
fn signed_word() {
    check(false, false);
}
#[test]
fn unsigned_word() {
    check(true, false);
}
#[test]
fn signed_halfwords() {
    check(false, true);
}
#[test]
fn unsigned_halfwords() {
    check(true, true);
}

#[test]
fn every_register_pair_preserves_neighbors_and_sticky_flags() {
    let data = include_bytes!("../fixtures/isa_oracle_saturate.bin");
    let registers: Vec<u32> = (0..13).chain([14]).collect();
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for block in data.chunks_exact(128 * 24) {
            let base = u32::from_le_bytes(block[..4].try_into().unwrap());
            if lanes(base) && !cfg!(feature = "has-dsp-ext") {
                continue;
            }
            let width = base & 31;
            let amount = ((base >> 12) & 7) * 4 + ((base >> 6) & 3);
            if ![0, 15, 31].contains(&width) || ![0, 1, 31].contains(&amount) {
                continue;
            }
            for sample in [1usize, 8, 15] {
                let record = &block[sample * 4 * 24..(sample * 4 + 1) * 24];
                let [_, input, control, result, status, _]: [u32; 6] = core::array::from_fn(|i| {
                    u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
                });
                for &d in &registers {
                    for &n in &registers {
                        let opcode = (base & !0x000f0f00) | (n << 16) | (d << 8);
                        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                        assert_eq!(
                            zmu_cortex_m::core::instruction::instruction_size(&instruction),
                            4
                        );
                        for i in 0u8..15 {
                            cpu.set_r(Reg::from(i), 0xdeadbeef ^ (u32::from(i) * 0x01010101));
                        }
                        cpu.set_r(Reg::from(n as u8), input);
                        cpu.psr.value = control | (1 << 24);
                        let before: [u32; 15] =
                            core::array::from_fn(|i| cpu.get_r(Reg::from(i as u8)));
                        if compiled {
                            cpu.execute_compiled(&instruction, 4);
                        } else {
                            cpu.execute(&instruction, 4);
                        }
                        assert_eq!(
                            (cpu.get_r(Reg::from(d as u8)), cpu.psr.value & FLAGS),
                            (result, status & FLAGS),
                            "{opcode:08x}"
                        );
                        for (i, prior) in before.into_iter().enumerate() {
                            if i != d as usize {
                                assert_eq!(
                                    cpu.get_r(Reg::from(i as u8)),
                                    prior,
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

#[test]
fn invalid_registers_and_reserved_lane_bits_are_undefined() {
    let cpu = Processor::new();
    for base in [
        0xf3010000, 0xf3210040, 0xf3810000, 0xf3a10040, 0xf3210007, 0xf3a10007,
    ] {
        for bad in [13u32, 15] {
            for shift in [8, 16] {
                let opcode = (base & !(15 << shift)) | (bad << shift);
                assert!(
                    matches!(
                        cpu.decode(ThumbCode::Thumb32 { opcode }),
                        Instruction::UDF { .. }
                    ),
                    "{opcode:08x}"
                );
            }
        }
    }
    for opcode in [0xf3210010, 0xf3a10010] {
        assert!(matches!(
            cpu.decode(ThumbCode::Thumb32 { opcode }),
            Instruction::UDF { .. }
        ));
    }
}

#[test]
fn halfword_forms_require_dsp_and_skipped_saturation_preserves_all_state() {
    for opcode in [0xf3010017, 0xf3810017, 0xf3210007, 0xf3a10007] {
        let mut cpu = Processor::new();
        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
        assert_eq!(
            !matches!(instruction, Instruction::UDF { .. }),
            !lanes(opcode) || cfg!(feature = "has-dsp-ext")
        );
        if matches!(instruction, Instruction::UDF { .. }) {
            continue;
        }
        cpu.set_r(Reg::R0, 0x12345678);
        cpu.set_r(Reg::R1, 0x80008000);
        cpu.psr.value = 0x51050000;
        let it = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf18 });
        cpu.execute(&it, 2);
        let before = cpu.psr.value & FLAGS;
        cpu.execute(&instruction, 4);
        assert_eq!(
            (cpu.get_r(Reg::R0), cpu.get_r(Reg::R1)),
            (0x12345678, 0x80008000)
        );
        assert_eq!(
            (cpu.psr.value & FLAGS, cpu.get_itstate(), cpu.cfsr, cpu.hfsr),
            (before, 0, 0, 0)
        );
    }
}
