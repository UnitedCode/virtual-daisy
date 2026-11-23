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

const CODE: u32 = 0x00200000;
const RECORDS: &[u8] = include_bytes!("../fixtures/control.bin");

fn fixture(flags: u32) -> Processor {
    let mut bytes = [0u8; 512];
    for (at, value) in [
        (0, 0x2001fff0u32),
        (4, 0x101),
        (12, 0xc1),
        (16, 0xc1),
        (24, 0xc1),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut cpu = Processor::new();
    cpu.flash_memory(bytes.len(), &bytes);
    cpu.msp = 0x2001fff0;
    cpu.psr.value = flags | (1 << 24);
    cpu.r0_12[0] = 0xdeadbeef;
    cpu.branch_write_pc(CODE);
    cpu
}

fn execute(cpu: &mut Processor, opcode: u32, width: u32, compiled: bool) {
    let instruction = cpu.decode(if width == 4 {
        ThumbCode::Thumb32 { opcode }
    } else {
        ThumbCode::Thumb16 {
            opcode: opcode as u16,
        }
    });
    if compiled {
        cpu.execute_compiled(&instruction, width as usize);
    } else {
        cpu.execute(&instruction, width as usize);
    }
}

fn compare(branches: bool) {
    assert_eq!(RECORDS.len(), 9664 * 20);
    for compiled in [false, true] {
        for raw in RECORDS.chunks_exact(20) {
            let words: Vec<_> = raw
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            let [kind, opcode, flags, result, final_flags] = words[..] else {
                unreachable!()
            };
            if (kind == 0) != branches {
                continue;
            }
            let mut cpu = fixture(flags);
            if kind & 0x10 != 0 {
                execute(&mut cpu, 0xbf0c, 2, compiled);
            }
            execute(
                &mut cpu,
                opcode,
                if branches { 4 } else { kind & 0xf },
                compiled,
            );
            let actual = if branches {
                cpu.get_r(Reg::PC)
            } else {
                execute(&mut cpu, 0x4678, 2, compiled);
                cpu.r0_12[0]
            };
            assert_eq!(
                actual, result,
                "kind={kind:x}, opcode={opcode:08x}, flags={flags:08x}, compiled={compiled}"
            );
            assert_eq!(cpu.psr.value & 0xf80f0000, final_flags);
            assert_eq!(cpu.cfsr, 0, "opcode={opcode:08x}");
            assert_eq!(&cpu.r0_12[1..], &[0; 12]);
        }
    }
}

#[test]
fn conditional_wide_branches_match_qemu_at_every_condition_and_high_offset_bit() {
    compare(true);
}

#[test]
fn unallocated_hints_match_qemu_with_and_without_an_it_block() {
    compare(false);
}

#[test]
fn reserved_wide_hint_groups_and_branch_conditions_are_undefined() {
    let cpu = Processor::new();
    for group in 1..8u32 {
        for option in 0..256u32 {
            let opcode = 0xf3af8000 | (group << 8) | option;
            assert!(
                matches!(
                    cpu.decode(ThumbCode::Thumb32 { opcode }),
                    Instruction::UDF { thumb32: true, .. }
                ),
                "{opcode:08x}"
            );
        }
    }
    for opcode in [0xf3a08000, 0xf3c08000] {
        assert!(
            matches!(
                cpu.decode(ThumbCode::Thumb32 { opcode }),
                Instruction::UDF { thumb32: true, .. }
            ),
            "{opcode:08x}"
        );
    }
    for opcode in [0xf3af8002, 0xf3af8003, 0xf3af8004, 0xf3808814, 0xf3ef8000] {
        assert!(
            !matches!(
                cpu.decode(ThumbCode::Thumb32 { opcode }),
                Instruction::UDF { .. }
            ),
            "{opcode:08x}"
        );
    }
}
