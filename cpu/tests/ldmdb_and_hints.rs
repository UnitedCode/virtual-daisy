#![cfg(any(feature = "armv7m", feature = "armv7em"))]
use zmu_cortex_m::{
    Processor,
    bus::Bus,
    core::{
        instruction::Instruction,
        register::{BaseReg, Reg},
        thumb::ThumbCode,
    },
    decoder::Decoder,
    executor::Executor,
};
fn fixture() -> Processor {
    let mut bytes = vec![0; 512];
    for (at, value) in [(0, 0x2001fff0u32), (4, 0x101), (12, 0xc1), (24, 0xc1)] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut cpu = Processor::new();
    cpu.flash_memory(bytes.len(), &bytes);
    cpu.msp = 0x2001fff0;
    cpu
}
fn execute(cpu: &mut Processor, opcode: u32, compiled: bool) {
    let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
    if compiled {
        cpu.execute_compiled(&instruction, 4);
    } else {
        cpu.execute(&instruction, 4);
    }
}
fn check_load(cpu: &mut Processor, rn: u32, mask: u32, wback: bool, compiled: bool) {
    let end = 0x20000080u32;
    let count = mask.count_ones();
    let start = end - count * 4;
    cpu.branch_write_pc(0x100);
    cpu.psr.value = 0xa90a0000;
    for reg in 0..15u8 {
        cpu.set_r(Reg::from(reg), 0x13570000 + u32::from(reg) * 4);
    }
    cpu.set_r(Reg::from(rn as u8), end);
    let before: Vec<_> = (0..15u8).map(|r| cpu.get_r(Reg::from(r))).collect();
    let values: Vec<_> = (0..count)
        .map(|slot| {
            if mask & 0x8000 != 0 && slot == count - 1 {
                0x301
            } else {
                0xa5000100 + slot * 4
            }
        })
        .collect();
    for (i, &value) in values.iter().enumerate() {
        cpu.write32(start + i as u32 * 4, value).unwrap();
    }
    cpu.write32(start - 4, 0x55aac33c).unwrap();
    cpu.write32(end, 0xa55a3cc3).unwrap();
    let opcode = 0xe9100000 | (u32::from(wback) << 21) | (rn << 16) | mask;
    execute(cpu, opcode, compiled);
    assert_eq!(cpu.cfsr, 0, "{opcode:08x}");
    for reg in 0..15u32 {
        let expected = if mask & (1 << reg) != 0 {
            values[(mask & ((1 << reg) - 1)).count_ones() as usize]
        } else if reg == rn && wback {
            start
        } else {
            before[reg as usize]
        };
        assert_eq!(
            cpu.get_r(Reg::from(reg as u8)),
            expected,
            "{opcode:08x} R{reg}"
        );
    }
    for (i, &value) in values.iter().enumerate() {
        assert_eq!(cpu.read32(start + i as u32 * 4).unwrap(), value);
    }
    assert_eq!(cpu.read32(start - 4).unwrap(), 0x55aac33c);
    assert_eq!(cpu.read32(end).unwrap(), 0xa55a3cc3);
    assert_eq!(cpu.psr.value, 0xa90a0000);
    assert_eq!(
        cpu.get_r(Reg::PC),
        if mask & 0x8000 != 0 { 0x304 } else { 0x108 }
    );
}
#[test]
fn ldmdb_covers_every_legal_register_list_with_and_without_writeback() {
    let mut checked = 0;
    for compiled in [false, true] {
        let mut cpu = fixture();
        for mask in 0..=65535u32 {
            if mask & 0x2000 != 0 || mask & 0xc000 == 0xc000 || mask.count_ones() < 2 {
                continue;
            }
            for wback in [false, true] {
                if wback && mask & (1 << 12) != 0 {
                    continue;
                }
                check_load(&mut cpu, 12, mask, wback, compiled);
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 73666);
}
#[test]
fn ldmdb_covers_every_base_including_sp_and_lr_and_base_in_list_without_writeback() {
    let mut checked = 0;
    for compiled in [false, true] {
        let mut cpu = fixture();
        for rn in 0..15u32 {
            for mask in [3, 0x1001, 0x4001, 0x8001, 0x1fff] {
                for wback in [false, true] {
                    if wback && mask & (1 << rn) != 0 {
                        continue;
                    }
                    check_load(&mut cpu, rn, mask, wback, compiled);
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 260);
}
#[test]
fn ldmdb_rejects_invalid_lists_bases_and_writeback_aliases() {
    let cpu = fixture();
    for opcode in [0xe9100000, 0xe9100001, 0xe91f0003, 0xe910c003, 0xe9300003] {
        assert!(
            matches!(
                cpu.decode(ThumbCode::Thumb32 { opcode }),
                Instruction::UDF { .. }
            ),
            "{opcode:08x}"
        );
    }
}
#[test]
fn ldmdb_alignment_faults_before_register_loads_or_writeback() {
    for compiled in [false, true] {
        let mut cpu = fixture();
        cpu.branch_write_pc(0x100);
        cpu.r0_12[0] = 0x12345678;
        cpu.r0_12[1] = 0xabcdef01;
        cpu.r0_12[8] = 0x20000082;
        execute(&mut cpu, 0xe9380003, compiled);
        assert_ne!(cpu.cfsr & (1 << 24), 0);
        assert_eq!(&cpu.r0_12[..2], &[0x12345678, 0xabcdef01]);
        assert_eq!(cpu.r0_12[8], 0x20000082);
    }
}
#[test]
fn all_debug_hint_options_preserve_state_and_retire_without_faults() {
    for compiled in [false, true] {
        for option in 0..16u32 {
            let mut cpu = fixture();
            cpu.branch_write_pc(0x100);
            cpu.psr.value = 0xa90a0000;
            cpu.r0_12.fill(0xabcdef01);
            let sp = cpu.msp;
            execute(&mut cpu, 0xf3af80f0 | option, compiled);
            assert_eq!(cpu.cfsr, 0, "option={option}");
            assert_eq!(cpu.r0_12, [0xabcdef01; 13]);
            assert_eq!(cpu.psr.value, 0xa90a0000);
            assert_eq!(cpu.msp, sp);
            assert_eq!(cpu.get_r(Reg::PC), 0x108);
        }
    }
}
#[test]
fn skipped_ldmdb_and_debug_hints_leave_state_unchanged() {
    for compiled in [false, true] {
        for opcode in [0xe9380003, 0xf3af80f0, 0xf3af80ff] {
            let mut cpu = fixture();
            cpu.branch_write_pc(0x100);
            cpu.psr.value = 0xa90a0000;
            cpu.r0_12.fill(0xfffffffc);
            let it = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf08 });
            cpu.execute(&it, 2);
            execute(&mut cpu, opcode, compiled);
            assert_eq!(cpu.cfsr, 0);
            assert_eq!(cpu.r0_12, [0xfffffffc; 13]);
            assert_eq!(cpu.psr.value, 0xa90a0000);
        }
    }
}
