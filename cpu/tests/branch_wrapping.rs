use zmu_cortex_m::{
    Processor,
    core::{
        register::{BaseReg, Reg},
        thumb::ThumbCode,
    },
    decoder::Decoder,
    executor::Executor,
};

fn execute(
    start: u32,
    opcode: u32,
    wide: bool,
    flags: u32,
    operand: u32,
    compiled: bool,
) -> Processor {
    let mut cpu = Processor::new();
    cpu.psr.value = flags | (1 << 24);
    cpu.r0_12[0] = operand;
    cpu.r0_12[1] = 0x101;
    cpu.branch_write_pc(start);
    let instruction = cpu.decode(if wide {
        ThumbCode::Thumb32 { opcode }
    } else {
        ThumbCode::Thumb16 {
            opcode: opcode as u16,
        }
    });
    if compiled {
        cpu.execute_compiled(&instruction, if wide { 4 } else { 2 });
    } else {
        cpu.execute(&instruction, if wide { 4 } else { 2 });
    }
    assert_eq!(cpu.cfsr, 0);
    assert_eq!(cpu.psr.value & 0xf80f0000, flags);
    cpu
}

#[test]
fn relocated_qemu_branch_results_preserve_32_bit_pc_arithmetic() {
    for record in include_bytes!("../fixtures/control.bin").chunks_exact(20) {
        let word = |n: usize| u32::from_le_bytes(record[4 * n..4 * n + 4].try_into().unwrap());
        if word(0) != 0 {
            continue;
        }
        for start in [0, 0x100, 0x7ffffff8, 0x80000000, 0xfffffff8, 0xfffffffc] {
            let expected = word(3).wrapping_sub(0x00200000).wrapping_add(start);
            for compiled in [false, true] {
                let cpu = execute(start, word(1), true, word(2), 0, compiled);
                assert_eq!(
                    cpu.get_r(Reg::PC),
                    expected,
                    "pc={start:08x} opcode={:08x}",
                    word(1)
                );
            }
        }
    }
}

#[test]
fn unconditional_branches_and_links_wrap_at_signed_and_unsigned_boundaries() {
    for compiled in [false, true] {
        for start in [0, 0x7ffffff8, 0xfffffff8, 0xfffffffc] {
            for (opcode, wide, link) in [
                (0xe004, false, false),
                (0xf000b804, true, false),
                (0xf000f804, true, true),
            ] {
                let cpu = execute(start, opcode, wide, 0xa0000000, 0, compiled);
                assert_eq!(cpu.get_r(Reg::PC), start.wrapping_add(16));
                if link {
                    assert_eq!(cpu.get_r(Reg::LR), start.wrapping_add(4) | 1);
                }
            }
            let cpu = execute(start, 0x4788, false, 0xa0000000, 0, compiled); // BLX R1.
            assert_eq!(cpu.get_r(Reg::PC), 0x104);
            assert_eq!(cpu.get_r(Reg::LR), start.wrapping_add(2) | 1);
        }
    }
}

#[test]
fn compare_and_branch_wraps_for_every_immediate_and_taken_state() {
    for compiled in [false, true] {
        for nonzero in [false, true] {
            for value in [0, 1] {
                for immediate in (0..=126u32).step_by(2) {
                    let opcode = 0xb100
                        | (u32::from(nonzero) << 11)
                        | ((immediate & 0x40) << 3)
                        | ((immediate & 0x3e) << 2);
                    let start: u32 = 0xfffffff8;
                    let cpu = execute(start, opcode, false, 0xa0000000, value, compiled);
                    let expected_pc = if (value != 0) == nonzero {
                        start.wrapping_add(8).wrapping_add(immediate)
                    } else {
                        start.wrapping_add(6)
                    };
                    assert_eq!(cpu.get_r(Reg::PC), expected_pc, "{opcode:04x}");
                    assert_eq!(cpu.r0_12[0], value);
                }
            }
        }
    }
}
