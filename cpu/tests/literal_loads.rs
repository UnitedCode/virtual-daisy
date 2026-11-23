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

fn fixture() -> (Processor, Vec<u8>) {
    let mut bytes: Vec<u8> = (0..0x4000u32)
        .map(|i| (i.wrapping_mul(73) ^ (i >> 3) ^ 0xa5) as u8)
        .collect();
    for (at, word) in [(0, 0x2001fff0u32), (4, 0x2001), (12, 0x401), (24, 0x401)] {
        bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    let mut cpu = Processor::new();
    cpu.flash_memory(bytes.len(), &bytes);
    cpu.msp = 0x2001fff0;
    (cpu, bytes)
}

fn execute(cpu: &mut Processor, opcode: u32, compiled: bool) {
    let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
    if compiled {
        cpu.execute_compiled(&instruction, 4);
    } else {
        cpu.execute(&instruction, 4);
    }
}

fn scalar_value(bytes: &[u8], address: usize, width: usize, signed: bool) -> u32 {
    match (width, signed) {
        (1, false) => u32::from(bytes[address]),
        (1, true) => bytes[address] as i8 as i32 as u32,
        (2, false) => u32::from(u16::from_le_bytes(
            bytes[address..address + 2].try_into().unwrap(),
        )),
        (2, true) => {
            i16::from_le_bytes(bytes[address..address + 2].try_into().unwrap()) as i32 as u32
        }
        (4, false) => u32::from_le_bytes(bytes[address..address + 4].try_into().unwrap()),
        _ => unreachable!(),
    }
}

#[test]
fn scalar_literal_loads_cover_all_offsets_signs_and_both_pc_alignments() {
    for compiled in [false, true] {
        for (base, width, signed) in [
            (0xf89f0000u32, 1, false),
            (0xf8bf0000, 2, false),
            (0xf99f0000, 1, true),
            (0xf9bf0000, 2, true),
            (0xf8df0000, 4, false),
        ] {
            for pc in [0x2000u32, 0x2002] {
                for add in [false, true] {
                    let (mut cpu, bytes) = fixture();
                    for immediate in 0..=4095u32 {
                        cpu.branch_write_pc(pc);
                        cpu.psr.value = 0xa90a0000;
                        cpu.r0_12[0] = 0xdeadbeef;
                        let address = if add {
                            0x2004 + immediate
                        } else {
                            0x2004 - immediate
                        } as usize;
                        let value = scalar_value(&bytes, address, width, signed);
                        let opcode = (base & !(1 << 23)) | (u32::from(add) << 23) | immediate;
                        execute(&mut cpu, opcode, compiled);
                        assert_eq!(cpu.cfsr, 0, "{opcode:08x} pc={pc:x}");
                        assert_eq!(cpu.r0_12[0], value, "{opcode:08x} pc={pc:x}");
                        assert_eq!(cpu.psr.value, 0xa90a0000);
                        assert_eq!(cpu.get_r(Reg::PC), pc + 8);
                    }
                }
            }
        }
    }
}

#[test]
fn doubleword_literals_cover_every_scaled_offset_and_both_pc_alignments() {
    for compiled in [false, true] {
        for pc in [0x2000u32, 0x2002] {
            for add in [false, true] {
                let (mut cpu, bytes) = fixture();
                for immediate in 0..=255u32 {
                    cpu.branch_write_pc(pc);
                    cpu.psr.value = 0xa90a0000;
                    let address = if add {
                        0x2004 + immediate * 4
                    } else {
                        0x2004 - immediate * 4
                    } as usize;
                    let expected = [
                        u32::from_le_bytes(bytes[address..address + 4].try_into().unwrap()),
                        u32::from_le_bytes(bytes[address + 4..address + 8].try_into().unwrap()),
                    ];
                    let opcode = 0xe95f0100 | (u32::from(add) << 23) | immediate;
                    execute(&mut cpu, opcode, compiled);
                    assert_eq!(cpu.cfsr, 0, "{opcode:08x} pc={pc:x}");
                    assert_eq!(&cpu.r0_12[..2], &expected);
                    assert_eq!(cpu.psr.value, 0xa90a0000);
                    assert_eq!(cpu.get_r(Reg::PC), pc + 8);
                }
            }
        }
    }
}

#[test]
fn table_branches_wrap_the_address_before_accessing_valid_ram() {
    for compiled in [false, true] {
        for (opcode, index) in [(0xe8d0f001, 0x20000004), (0xe8d0f011, 0x10000002)] {
            let (mut cpu, _) = fixture();
            cpu.branch_write_pc(0x2000);
            cpu.psr.value = 0xa90a0000;
            cpu.r0_12[0] = 0xfffffffc;
            cpu.r0_12[1] = index;
            zmu_cortex_m::bus::Bus::write32(&mut cpu, 0x20000000, 2).unwrap();
            execute(&mut cpu, opcode, compiled);
            assert_eq!(cpu.cfsr, 0);
            assert_eq!(cpu.get_r(Reg::PC), 0x200c);
            assert_eq!(&cpu.r0_12[..2], &[0xfffffffc, index]);
            assert_eq!(cpu.psr.value, 0xa90a0000);
        }
    }
}

#[test]
fn word_literal_offsets_are_not_mistaken_for_register_addressing() {
    for compiled in [false, true] {
        for pc in [0x2000u32, 0x2002] {
            let (mut cpu, bytes) = fixture();
            for immediate in [0, 1, 4, 64, 255, 4095] {
                cpu.branch_write_pc(pc);
                cpu.psr.value = 0xa90a0000;
                cpu.r0_12[0] = 0xdeadbeef;
                let address = (0x2004 - immediate) as usize;
                let expected = u32::from_le_bytes(bytes[address..address + 4].try_into().unwrap());
                execute(&mut cpu, 0xf85f0000 | immediate, compiled);
                assert_eq!(cpu.cfsr, 0, "offset={immediate} pc={pc:x}");
                assert_eq!(cpu.r0_12[0], expected);
            }
        }
    }
}

#[test]
fn literal_register_fields_select_general_registers_and_distinct_pairs() {
    let registers: Vec<Reg> = (0u8..13).chain([14u8]).map(Reg::from).collect();
    for compiled in [false, true] {
        let (mut cpu, bytes) = fixture();
        for (base, width, signed) in [
            (0xf89f0000u32, 1, false),
            (0xf8bf0000, 2, false),
            (0xf99f0000, 1, true),
            (0xf9bf0000, 2, true),
            (0xf8df0000, 4, false),
        ] {
            for &destination in &registers {
                for pc in [0x2000, 0x2002] {
                    for add in [false, true] {
                        for immediate in [0, 1, 2047, 4095] {
                            cpu.branch_write_pc(pc);
                            cpu.psr.value = 0xa90a0000;
                            for (i, &register) in registers.iter().enumerate() {
                                cpu.set_r(register, 0x13570000 + i as u32);
                            }
                            let address = if add {
                                0x2004 + immediate
                            } else {
                                0x2004 - immediate
                            } as usize;
                            let value = scalar_value(&bytes, address, width, signed);
                            let opcode = (base & !(1 << 23))
                                | (u32::from(add) << 23)
                                | ((destination as u32) << 12)
                                | immediate;
                            execute(&mut cpu, opcode, compiled);
                            assert_eq!(cpu.cfsr, 0, "{opcode:08x}");
                            for (i, &register) in registers.iter().enumerate() {
                                assert_eq!(
                                    cpu.get_r(register),
                                    if register == destination {
                                        value
                                    } else {
                                        0x13570000 + i as u32
                                    }
                                );
                            }
                            assert_eq!(cpu.psr.value, 0xa90a0000);
                        }
                    }
                }
            }
        }
        for &low in &registers {
            for &high in &registers {
                if low == high {
                    continue;
                }
                for pc in [0x2000, 0x2002] {
                    for add in [false, true] {
                        for immediate in [0, 1, 127, 255] {
                            cpu.branch_write_pc(pc);
                            cpu.psr.value = 0xa90a0000;
                            for (i, &register) in registers.iter().enumerate() {
                                cpu.set_r(register, 0x13570000 + i as u32);
                            }
                            let address = if add {
                                0x2004 + immediate * 4
                            } else {
                                0x2004 - immediate * 4
                            } as usize;
                            let expected = [
                                scalar_value(&bytes, address, 4, false),
                                scalar_value(&bytes, address + 4, 4, false),
                            ];
                            let opcode = 0xe95f0000
                                | (u32::from(add) << 23)
                                | ((low as u32) << 12)
                                | ((high as u32) << 8)
                                | immediate;
                            execute(&mut cpu, opcode, compiled);
                            assert_eq!(cpu.cfsr, 0, "{opcode:08x}");
                            for (i, &register) in registers.iter().enumerate() {
                                assert_eq!(
                                    cpu.get_r(register),
                                    if register == low {
                                        expected[0]
                                    } else if register == high {
                                        expected[1]
                                    } else {
                                        0x13570000 + i as u32
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
}

#[test]
fn word_literals_can_load_pc_and_halfword_literals_honor_alignment_trapping() {
    for compiled in [false, true] {
        for pc in [0x2000, 0x2002] {
            let (mut cpu, mut bytes) = fixture();
            bytes[0x2004..0x2008].copy_from_slice(&0x3001u32.to_le_bytes());
            cpu.flash_memory(bytes.len(), &bytes);
            cpu.branch_write_pc(pc);
            cpu.psr.value = 0xa90a0000;
            execute(&mut cpu, 0xf85ff000, compiled);
            assert_eq!(cpu.cfsr, 0);
            assert_eq!(cpu.get_r(Reg::PC), 0x3004);
            assert_eq!(cpu.psr.value, 0xa90a0000);
        }
        for opcode in [0xf8bf0001, 0xf9bf0001] {
            let (mut cpu, _) = fixture();
            cpu.branch_write_pc(0x2002);
            cpu.psr.value = 0x01000000;
            cpu.ccr = 8;
            cpu.r0_12[0] = 0xdeadbeef;
            execute(&mut cpu, opcode, compiled);
            assert_ne!(cpu.cfsr & (1 << 24), 0);
            assert_eq!(cpu.r0_12[0], 0xdeadbeef);
        }
    }
}
