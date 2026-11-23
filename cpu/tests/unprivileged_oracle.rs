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
const DATA: &[u8] = include_bytes!("../fixtures/isa_oracle_unprivileged.bin");
const BASE: u32 = 0x20001004;
#[path = "../fixtures/unprivileged_probe_cases.rs"]
mod probe_cases;

#[test]
fn standalone_guest_cases_and_initial_guards_are_pinned_to_independent_records() {
    for [opcode, result, lo, hi, status] in probe_cases::UNPRIVILEGED_CASES {
        assert!(DATA.as_chunks::<36>().0.iter().any(|row| {
            words(row)[..8]
                == [
                    opcode, 0x80008000, 0x80008000, 0xa80a0000, result, lo, hi, status,
                ]
        }));
    }
    for (offset, guards) in probe_cases::INITIAL_GUARDS.into_iter().enumerate() {
        assert!(DATA.as_chunks::<36>().0.iter().any(|row| {
            let w = words(row);
            w[0] == (0xf8510e00 | offset as u32) && w[1] == 0x80008000 && w[5..7] == guards
        }));
    }
}
fn words(row: &[u8]) -> [u32; 9] {
    core::array::from_fn(|i| u32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().unwrap()))
}
fn setup(cpu: &mut Processor, opcode: u32, seed: u32) {
    let offset = opcode & 255;
    for i in 0..8u32 {
        let index = offset + 3 + i;
        let byte = (seed.rotate_right((index % 4) * 8) as u8) ^ (index.wrapping_mul(37) as u8);
        cpu.write8(BASE + offset - 1 + i, byte).unwrap();
    }
}
fn memory(cpu: &Processor, opcode: u32) -> [u32; 2] {
    let bytes: [u8; 8] =
        core::array::from_fn(|i| cpu.read8(BASE + (opcode & 255) - 1 + i as u32).unwrap());
    [
        u32::from_le_bytes(bytes[..4].try_into().unwrap()),
        u32::from_le_bytes(bytes[4..].try_into().unwrap()),
    ]
}
#[test]
fn all_unprivileged_transfer_forms_decode_on_base_armv7m() {
    let cpu = Processor::new();
    for row in DATA.as_chunks::<36>().0.iter().take(64).step_by(8) {
        let opcode = words(row)[0];
        assert!(
            !matches!(
                cpu.decode(ThumbCode::Thumb32 { opcode }),
                Instruction::UDF { .. }
            ),
            "{opcode:08x}"
        );
    }
}
#[test]
fn results_memory_and_flags_match_independently_executed_instructions() {
    assert_eq!(DATA.len(), 262144 * 36);
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for row in DATA.as_chunks::<36>().0 {
            let [opcode, seed, source, control, result, lo, hi, status, magic] = words(row);
            assert_eq!(magic, 0x56525055);
            setup(&mut cpu, opcode, seed);
            cpu.set_r(Reg::R0, source);
            cpu.set_r(Reg::R1, BASE);
            cpu.psr.value = control | 1 << 24;
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            assert!(
                !matches!(instruction, Instruction::UDF { .. }),
                "missing {opcode:08x}"
            );
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_eq!(
                (
                    cpu.get_r(Reg::R0),
                    memory(&cpu, opcode),
                    cpu.psr.value & 0xf80f0000
                ),
                (result, [lo, hi], status),
                "{opcode:08x} seed {seed:x} source {source:x}"
            );
            assert_eq!(cpu.get_r(Reg::R1), BASE);
            assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
        }
    }
}
#[test]
fn every_legal_register_pair_sp_base_and_alias_matches_the_oracle() {
    let records: std::collections::BTreeMap<_, _> = DATA
        .as_chunks::<36>()
        .0
        .iter()
        .filter_map(|r| {
            let [op, seed, source, c, result, lo, hi, status, _] = words(r);
            if [0x80008000, 0x01234567].contains(&seed)
                && [0, 1, 127, 128, 255].contains(&(op & 255))
                && c == 0xa80a0000
            {
                Some(((op, seed, source), (result, [lo, hi], status)))
            } else {
                None
            }
        })
        .collect();
    let bases: [u32; 8] = [
        0xf8510e00, 0xf8110e00, 0xf8310e00, 0xf9110e00, 0xf9310e00, 0xf8410e00, 0xf8010e00,
        0xf8210e00,
    ];
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for base in bases {
            for offset in [0, 1, 127, 128, 255] {
                for seed in [0x80008000, 0x01234567] {
                    for t in (0..13).chain([14]) {
                        for n in 0..15 {
                            let opcode = (base & !0x000ff000) | (n << 16) | (t << 12) | offset;
                            setup(&mut cpu, opcode, seed);
                            cpu.r0_12.fill(0xdeadbeef);
                            cpu.set_r(Reg::SP, 0x2001f000);
                            cpu.set_r(Reg::LR, 0xabcd1234);
                            cpu.set_r(Reg::from(t as u8), seed);
                            cpu.set_r(Reg::from(n as u8), BASE);
                            let before: [u32; 15] =
                                core::array::from_fn(|i| cpu.get_r(Reg::from(i as u8)));
                            let source = before[t as usize];
                            let expected = records[&(base | offset, seed, source)];
                            cpu.psr.value = 0xa90a0000;
                            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                            if compiled {
                                cpu.execute_compiled(&instruction, 4);
                            } else {
                                cpu.execute(&instruction, 4);
                            }
                            assert_eq!(
                                (
                                    cpu.get_r(Reg::from(t as u8)),
                                    memory(&cpu, opcode),
                                    cpu.psr.value & 0xf80f0000
                                ),
                                expected,
                                "{opcode:08x}"
                            );
                            for (i, old) in before.into_iter().enumerate() {
                                if i != t as usize {
                                    assert_eq!(
                                        cpu.get_r(Reg::from(i as u8)),
                                        old,
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
}

#[derive(Clone)]
struct PermissionBus {
    writes: std::rc::Rc<std::cell::Cell<usize>>,
    privileges: std::rc::Rc<std::cell::RefCell<Vec<bool>>>,
}
impl Bus for PermissionBus {
    fn validate_access(
        &self,
        address: u32,
        _width: usize,
        _kind: u8,
        privileged: bool,
        _bypass: bool,
    ) -> Result<(), zmu_cortex_m::core::fault::Fault> {
        if (0x24000000..0x24000004).contains(&address) {
            self.privileges.borrow_mut().push(privileged);
            if !privileged {
                return Err(zmu_cortex_m::core::fault::Fault::DAccViol);
            }
        }
        Ok(())
    }
    fn read8(&self, _: u32) -> Result<u8, zmu_cortex_m::core::fault::Fault> {
        Ok(0x80)
    }
    fn read16(&self, _: u32) -> Result<u16, zmu_cortex_m::core::fault::Fault> {
        Ok(0x8000)
    }
    fn read32(&mut self, _: u32) -> Result<u32, zmu_cortex_m::core::fault::Fault> {
        Ok(0x80000000)
    }
    fn write8(&mut self, _: u32, _: u8) -> Result<(), zmu_cortex_m::core::fault::Fault> {
        self.writes.set(self.writes.get() + 1);
        Ok(())
    }
    fn write16(&mut self, _: u32, _: u16) -> Result<(), zmu_cortex_m::core::fault::Fault> {
        self.writes.set(self.writes.get() + 1);
        Ok(())
    }
    fn write32(&mut self, _: u32, _: u32) -> Result<(), zmu_cortex_m::core::fault::Fault> {
        self.writes.set(self.writes.get() + 1);
        Ok(())
    }
    fn in_range(&self, a: u32) -> bool {
        (0x24000000..0x24000004).contains(&a)
    }
}
#[test]
fn unprivileged_stores_cannot_use_the_callers_privileged_permissions() {
    for compiled in [false, true] {
        for opcode in [0xf8410e00, 0xf8010e00, 0xf8210e00] {
            let mut cpu = Processor::new();
            let bus = PermissionBus {
                writes: Default::default(),
                privileges: Default::default(),
            };
            cpu.device(Some(Box::new(bus.clone())));
            cpu.set_r(Reg::SP, 0x2001f000);
            cpu.set_r(Reg::R0, 0xdeadbeef);
            cpu.set_r(Reg::R1, 0x24000000);
            cpu.psr.value = 1 << 24;
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_eq!(*bus.privileges.borrow(), [false], "{opcode:08x}");
            assert_eq!(bus.writes.get(), 0);
            assert_ne!(cpu.cfsr & 2, 0);
            cpu.write32(0x24000000, 0x12345678).unwrap();
            assert_eq!(bus.privileges.borrow().last(), Some(&true));
            assert_eq!(bus.writes.get(), 1);
        }
    }
}

#[test]
fn skipped_transfers_do_not_access_memory_or_change_registers() {
    for compiled in [false, true] {
        for opcode in [
            0xf8510e00, 0xf8110e00, 0xf8310e00, 0xf9110e00, 0xf9310e00, 0xf8410e00, 0xf8010e00,
            0xf8210e00,
        ] {
            let mut cpu = Processor::new();
            let bus = PermissionBus {
                writes: Default::default(),
                privileges: Default::default(),
            };
            cpu.device(Some(Box::new(bus.clone())));
            cpu.set_r(Reg::R0, 0xdeadbeef);
            cpu.set_r(Reg::R1, 0x24000000);
            cpu.psr.value = 0x51050000;
            let it = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf18 });
            cpu.execute(&it, 2);
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert!(bus.privileges.borrow().is_empty());
            assert_eq!(bus.writes.get(), 0);
            assert_eq!(
                (cpu.get_r(Reg::R0), cpu.get_r(Reg::R1)),
                (0xdeadbeef, 0x24000000)
            );
            assert_eq!((cpu.cfsr, cpu.hfsr, cpu.get_itstate()), (0, 0, 0));
            assert_eq!(cpu.psr.value & 0xf80f0000, 0x50050000);
        }
    }
}
#[test]
fn unprivileged_ppb_loads_fault_with_the_data_address() {
    for opcode in [0xf8510e00, 0xf8110e00, 0xf8310e00, 0xf9110e00, 0xf9310e00] {
        let mut cpu = Processor::new();
        cpu.set_r(Reg::SP, 0x2001f000);
        cpu.set_r(Reg::R0, 0xdeadbeef);
        cpu.set_r(Reg::R1, 0xe000ed00);
        cpu.psr.value = 1 << 24;
        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
        cpu.execute(&instruction, 4);
        assert_eq!(cpu.cfsr & 0x00010082, 0x82);
        assert_eq!(cpu.read32(0xe000ed34).unwrap(), 0xe000ed00);
        assert_eq!(cpu.get_r(Reg::R0), 0xdeadbeef);
    }
}
#[test]
fn ccr_traps_unaligned_halfword_word_transfers_before_writes() {
    for opcode in [0xf8510e01, 0xf8310e01, 0xf9310e01, 0xf8410e01, 0xf8210e01] {
        let mut cpu = Processor::new();
        setup(&mut cpu, opcode, 0x80008000);
        let before = memory(&cpu, opcode);
        cpu.write32(0xe000ed14, 0x208).unwrap();
        cpu.set_r(Reg::SP, 0x2001f000);
        cpu.set_r(Reg::R0, 0xdeadbeef);
        cpu.set_r(Reg::R1, BASE);
        cpu.psr.value = 1 << 24;
        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
        cpu.execute(&instruction, 4);
        assert_eq!(cpu.cfsr & 0x01010082, 1 << 24);
        assert_eq!(memory(&cpu, opcode), before);
        assert_eq!(cpu.get_r(Reg::R0), 0xdeadbeef);
    }
}
#[test]
fn user_stir_permission_preserves_disabled_pending_bits() {
    use zmu_cortex_m::core::{exception::ExceptionHandling, reset::Reset};
    for allow in [false, true] {
        let mut cpu = Processor::new();
        cpu.reset().unwrap();
        cpu.psr.value = 1 << 24;
        if allow {
            cpu.write32(0xe000ed14, 0x202).unwrap();
        }
        cpu.set_r(Reg::SP, 0x2001f000);
        cpu.set_r(Reg::R0, 5);
        cpu.set_r(Reg::R1, 0xe000ef00);
        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode: 0xf8410e00 });
        cpu.execute(&instruction, 4);
        if allow {
            assert_eq!(cpu.cfsr, 0);
            assert_eq!(cpu.read32(0xe000e200).unwrap(), 1 << 5);
            assert!(cpu.get_pending_exception().is_none());
            cpu.write32(0xe000e100, 1 << 5).unwrap();
            assert!(cpu.get_pending_exception().is_some());
        } else {
            assert_eq!(cpu.cfsr & 0x82, 0x82);
            assert_eq!(cpu.read32(0xe000e200).unwrap(), 0);
        }
    }
}
#[test]
fn invalid_raw_transfers_fault_before_data_access() {
    for base in [
        0xf8510e00, 0xf8110e00, 0xf8310e00, 0xf9110e00, 0xf9310e00, 0xf8410e00, 0xf8010e00,
        0xf8210e00,
    ] {
        for (mask, bits) in [
            (0x0000f000, 13 << 12),
            (0x0000f000, 15 << 12),
            (0x000f0000, 15 << 16),
        ] {
            let opcode = (base & !mask) | bits;
            let mut cpu = Processor::new();
            let bus = PermissionBus {
                writes: Default::default(),
                privileges: Default::default(),
            };
            cpu.device(Some(Box::new(bus.clone())));
            cpu.set_r(Reg::SP, 0x2001f000);
            cpu.psr.value = 1 << 24;
            cpu.execute(&Instruction::Unprivileged { opcode }, 4);
            assert_ne!(cpu.cfsr & (1 << 16), 0);
            assert!(bus.privileges.borrow().is_empty());
            assert_eq!(bus.writes.get(), 0);
        }
    }
}
#[test]
fn user_stir_permission_can_be_revoked_after_a_successful_transfer() {
    use zmu_cortex_m::core::reset::Reset;
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        cpu.reset().unwrap();
        cpu.psr.value = 1 << 24;
        cpu.set_r(Reg::SP, 0x2001f000);
        cpu.set_r(Reg::R0, 5);
        cpu.set_r(Reg::R1, 0xe000ef00);
        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode: 0xf8410e00 });
        cpu.write32(0xe000ed14, 0x202).unwrap();
        if compiled {
            cpu.execute_compiled(&instruction, 4);
        } else {
            cpu.execute(&instruction, 4);
        }
        assert_eq!(cpu.read32(0xe000e200).unwrap(), 32);
        cpu.write32(0xe000e280, 32).unwrap();
        cpu.write32(0xe000ed14, 0x200).unwrap();
        if compiled {
            cpu.execute_compiled(&instruction, 4);
        } else {
            cpu.execute(&instruction, 4);
        }
        assert_eq!(cpu.cfsr & 0x82, 0x82);
        assert_eq!(cpu.read32(0xe000e200).unwrap(), 0);
    }
}
