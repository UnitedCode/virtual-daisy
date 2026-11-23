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
#[path = "../fixtures/psr_probe_cases.rs"]
mod probe_cases;
#[test]
fn standalone_psr_cases_are_pinned_to_the_independent_records() {
    let data = include_bytes!("../fixtures/isa_oracle_psr.bin");
    for expected in probe_cases::CASES {
        assert!(data.as_chunks::<32>().0.iter().any(|record| {
            let row: [u32; 7] = core::array::from_fn(|i| {
                u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
            });
            row == expected
        }));
    }
}
#[test]
fn msr_decodes_each_apsr_field_mask() {
    let cpu = Processor::new();
    for mask in 1..=3 {
        let opcode = 0xf3808000 | (mask << 10);
        assert!(
            matches!(cpu.decode(ThumbCode::Thumb32{opcode}),Instruction::MSR_reg{params} if u32::from(params.mask)==mask),
            "{opcode:08x}"
        );
    }
}

#[test]
fn status_register_transfers_reject_sp_and_pc_operands() {
    let cpu = Processor::new();
    for reg in [13u32, 15] {
        for sysm in [0, 3, 6, 20] {
            for opcode in [
                0xf3ef8000 | (reg << 8) | sysm,
                0xf3808800 | (reg << 16) | sysm,
            ] {
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
}
#[test]
fn reading_epsr_returns_zero_without_ge_or_thumb_state() {
    let mut cpu = Processor::new();
    cpu.psr.value = 0xa90a0000;
    let instruction = cpu.decode(ThumbCode::Thumb32 { opcode: 0xf3ef8006 });
    cpu.execute(&instruction, 4);
    assert_eq!(cpu.get_r(Reg::R0), 0);
}

#[test]
fn masked_alias_writes_and_unprivileged_access_match_executed_arm_records() {
    let data = include_bytes!("../fixtures/isa_oracle_psr.bin");
    assert_eq!(data.len(), 768 * 32);
    for compiled in [false, true] {
        for record in data.as_chunks::<32>().0 {
            let mut cpu = Processor::new();
            let [opcode, input, control, unpriv, apsr, xpsr, epsr, magic]: [u32; 8] =
                core::array::from_fn(|i| {
                    u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
                });
            assert_eq!(magic, 0x52535041);
            let visible = if cfg!(feature = "has-dsp-ext") {
                u32::MAX
            } else {
                !0x000f0000
            };
            cpu.psr.value = (control & visible) | (1 << 24);
            cpu.set_r(Reg::R0, unpriv);
            let set_control = cpu.decode(ThumbCode::Thumb32 { opcode: 0xf3808814 });
            cpu.execute(&set_control, 4);
            let get_control = cpu.decode(ThumbCode::Thumb32 { opcode: 0xf3ef8014 });
            cpu.execute(&get_control, 4);
            assert_eq!(cpu.get_r(Reg::R0) & 1, unpriv);
            cpu.set_r(Reg::R1, input);
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            assert!(
                matches!(instruction, Instruction::MSR_reg { .. }),
                "{opcode:08x}"
            );
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_ne!(cpu.psr.value & (1 << 24), 0, "MSR cannot clear EPSR.T");
            for (sysm, expected) in [(0, apsr), (3, xpsr), (6, epsr)] {
                let read = cpu.decode(ThumbCode::Thumb32 {
                    opcode: 0xf3ef8000 | sysm,
                });
                if compiled {
                    cpu.execute_compiled(&read, 4);
                } else {
                    cpu.execute(&read, 4);
                }
                assert_eq!(
                    cpu.get_r(Reg::R0),
                    expected & visible,
                    "{opcode:08x} input={input:08x} control={control:08x} unpriv={unpriv} read={sysm}"
                );
            }
            assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
        }
    }
}
