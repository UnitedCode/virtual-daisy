#![cfg(feature = "has-fp")]
use zmu_cortex_m::{
    Processor,
    core::{
        instruction::{Instruction, instruction_size},
        thumb::ThumbCode,
    },
    decoder::Decoder,
    executor::Executor,
};

#[test]
fn fused_register_aliases_keep_all_sources_and_the_other_registers() {
    let data = include_bytes!("../fixtures/isa_oracle_fused.bin");
    for precision in [32, 64] {
        if precision == 64 && !cfg!(feature = "fpv5-d16") {
            continue;
        }
        for operation in 0..4 {
            let words = data
                .as_chunks::<52>()
                .0
                .iter()
                .map(|record| {
                    core::array::from_fn::<u32, 13, _>(|i| {
                        u32::from_le_bytes(record[i * 4..i * 4 + 4].try_into().unwrap())
                    })
                })
                .find(|r| {
                    r[0] == precision
                        && r[7] == operation
                        && r[8] == 0xa000_0000
                        && if precision == 32 {
                            r[1] == 1f32.to_bits() && r[3] == r[1] && r[5] == r[1]
                        } else {
                            r[1] == 0
                                && r[2] == 0x3ff0_0000
                                && r[3] == 0
                                && r[4] == r[2]
                                && r[5] == 0
                                && r[6] == r[2]
                        }
                })
                .expect("executed all-ones operands");
            let base = if precision == 32 {
                [0xeea00a81, 0xeea00ac1, 0xee900a81, 0xee900ac1]
            } else {
                [0xeea10b02, 0xeea10b42, 0xee910b02, 0xee910b42]
            }[operation as usize];
            for [d, n, m] in [
                [0, 1, 2],
                [31, 30, 29],
                [0, 0, 1],
                [0, 1, 0],
                [0, 0, 0],
                [1, 0, 1],
                [15, 15, 14],
                [15, 14, 15],
                [15, 15, 15],
            ] {
                if precision == 64 && [d, n, m].iter().any(|&r| r >= 16) {
                    continue;
                }
                let fields = if precision == 32 {
                    ((d & 1) << 22)
                        | ((d >> 1) << 12)
                        | ((n & 1) << 7)
                        | ((n >> 1) << 16)
                        | ((m & 1) << 5)
                        | (m >> 1)
                } else {
                    (d << 12) | (n << 16) | m
                };
                let opcode = (base & !0x004f_f0af) | fields;
                for compiled in [false, true] {
                    let mut cpu = Processor::new();
                    cpu.cpacr = 0x00f0_0000;
                    cpu.fpscr = words[8];
                    cpu.fp_regs.fill(0xdead_beef);
                    for reg in [d, n, m] {
                        if precision == 32 {
                            cpu.fp_regs[reg as usize] = 1f32.to_bits();
                        } else {
                            cpu.fp_regs[2 * reg as usize] = 0;
                            cpu.fp_regs[2 * reg as usize + 1] = 0x3ff0_0000;
                        }
                    }
                    let before = cpu.fp_regs;
                    let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                    assert!(
                        !matches!(instruction, Instruction::UDF { .. }),
                        "{opcode:08x}"
                    );
                    assert_eq!(instruction_size(&instruction), 4);
                    if compiled {
                        cpu.execute_compiled(&instruction, 4);
                    } else {
                        cpu.execute(&instruction, 4);
                    }
                    let start = if precision == 32 {
                        d as usize
                    } else {
                        2 * d as usize
                    };
                    let count = if precision == 32 { 1 } else { 2 };
                    assert_eq!(cpu.fp_regs[start], words[9], "{instruction:?}");
                    if count == 2 {
                        assert_eq!(cpu.fp_regs[start + 1], words[10]);
                    }
                    assert_eq!(cpu.fpscr, words[11]);
                    assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
                    for (i, &value) in before.iter().enumerate() {
                        if !(start..start + count).contains(&i) {
                            assert_eq!(cpu.fp_regs[i], value);
                        }
                    }
                }
            }
        }
    }
}
