#![cfg(any(feature = "armv7m", feature = "armv7em"))]
use zmu_cortex_m::{
    Processor,
    core::{
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

fn adr(rd: u32, immediate: u32, add: bool) -> u32 {
    (if add { 0xf20f0000 } else { 0xf2af0000 })
        | ((immediate & 0x800) << 15)
        | ((immediate & 0x700) << 4)
        | (rd << 8)
        | (immediate & 0xff)
}

#[test]
fn wide_adr_covers_all_immediates_destinations_signs_and_pc_alignments() {
    for compiled in [false, true] {
        let mut cpu = fixture();
        for rd in (0..13u32).chain([14]) {
            for pc in [0x100u32, 0x102, 0xfffff000, 0xfffff002] {
                for add in [false, true] {
                    for immediate in 0..4096 {
                        cpu.branch_write_pc(pc);
                        cpu.psr.value = 0xa90a0000;
                        cpu.set_r(Reg::from(rd as u8), 0xdeadbeef);
                        let base = (pc + 4) & !3;
                        let expected = if add {
                            base.wrapping_add(immediate)
                        } else {
                            base.wrapping_sub(immediate)
                        };
                        let opcode = adr(rd, immediate, add);
                        execute(&mut cpu, opcode, compiled);
                        assert_eq!(cpu.cfsr, 0, "{opcode:08x}");
                        assert_eq!(cpu.get_r(Reg::from(rd as u8)), expected, "{opcode:08x}");
                        assert_eq!(cpu.psr.value, 0xa90a0000);
                        assert_eq!(cpu.get_r(Reg::PC), pc + 8);
                    }
                }
            }
        }
    }
}

fn reverse(opcode: u32, value: u32) -> u32 {
    let bytes = value.to_le_bytes();
    match opcode {
        0xfa90f080 => u32::from_le_bytes([bytes[3], bytes[2], bytes[1], bytes[0]]),
        0xfa90f090 => u32::from_le_bytes([bytes[1], bytes[0], bytes[3], bytes[2]]),
        0xfa90f0b0 => i16::from_le_bytes([bytes[1], bytes[0]]) as i32 as u32,
        _ => unreachable!(),
    }
}

#[test]
fn wide_reversals_cover_general_register_pairs_aliases_and_preserved_state() {
    let registers: Vec<_> = (0..13u8).chain([14]).map(Reg::from).collect();
    for compiled in [false, true] {
        let mut cpu = fixture();
        for opcode in [0xfa90f080u32, 0xfa90f090, 0xfa90f0b0] {
            for &rd in &registers {
                for &rm in &registers {
                    for value in [0, 1, u32::MAX, 0x80ff7f01, 0x12345678, 0x0180ff7f] {
                        cpu.branch_write_pc(0x100);
                        cpu.psr.value = 0xa90a0000;
                        for (i, &register) in registers.iter().enumerate() {
                            cpu.set_r(register, 0x13570000 + i as u32);
                        }
                        cpu.set_r(rm, value);
                        let before: Vec<_> = registers.iter().map(|&r| cpu.get_r(r)).collect();
                        let encoded = opcode | ((rm as u32) << 16) | ((rd as u32) << 8) | rm as u32;
                        execute(&mut cpu, encoded, compiled);
                        assert_eq!(cpu.cfsr, 0, "{encoded:08x}");
                        for (i, &register) in registers.iter().enumerate() {
                            assert_eq!(
                                cpu.get_r(register),
                                if register == rd {
                                    reverse(opcode, value)
                                } else {
                                    before[i]
                                }
                            );
                        }
                        assert_eq!(cpu.psr.value, 0xa90a0000);
                    }
                }
            }
        }
    }
}

#[test]
fn wide_revsh_checks_every_low_halfword_without_using_the_upper_half() {
    for compiled in [false, true] {
        let mut cpu = fixture();
        for value in 0..=65535u32 {
            cpu.branch_write_pc(0x100);
            cpu.psr.value = 0xa90a0000;
            cpu.r0_12[1] = value | 0xa53c0000;
            execute(&mut cpu, 0xfa91f0b1, compiled);
            assert_eq!(cpu.cfsr, 0);
            assert_eq!(cpu.r0_12[0], reverse(0xfa90f0b0, value));
            assert_eq!(cpu.psr.value, 0xa90a0000);
        }
    }
}

#[test]
fn wide_core_forms_reject_forbidden_registers_and_inconsistent_source_fields() {
    for opcode in [
        adr(13, 3, true),
        adr(15, 3, false),
        0xfa91fd81,
        0xfa9df08d,
        0xfa91f08f,
        0xfa91f082,
    ] {
        let cpu = fixture();
        assert!(
            matches!(
                cpu.decode(ThumbCode::Thumb32 { opcode }),
                zmu_cortex_m::core::instruction::Instruction::UDF { .. }
            ),
            "{opcode:08x}"
        );
    }
}

#[test]
fn skipped_wide_adr_and_reversals_leave_registers_and_flags_unchanged() {
    for compiled in [false, true] {
        for opcode in [
            adr(0, 4095, false),
            adr(14, 4095, true),
            0xfa91f081,
            0xfa91f091,
            0xfa91f0b1,
        ] {
            let mut cpu = fixture();
            cpu.branch_write_pc(0x100);
            cpu.psr.value = 0xa90a0000; // Z=0: EQ fails.
            let it = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf08 });
            cpu.execute(&it, 2);
            cpu.r0_12.fill(0x12345678);
            cpu.lr = 0xabcdef01;
            execute(&mut cpu, opcode, compiled);
            assert_eq!(cpu.r0_12, [0x12345678; 13]);
            assert_eq!(cpu.lr, 0xabcdef01);
            assert_eq!(cpu.psr.value, 0xa90a0000);
            assert_eq!(cpu.cfsr, 0);
        }
    }
}
