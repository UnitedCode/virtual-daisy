#![cfg(not(feature = "armv6m"))]

use zmu_cortex_m::{
    Processor,
    core::{
        exception::{Exception, ExceptionHandling},
        register::BaseReg,
        thumb::ThumbCode,
    },
    decoder::Decoder,
    executor::Executor,
};

fn execute(cpu: &mut Processor, opcode: ThumbCode) {
    let width = if matches!(opcode, ThumbCode::Thumb32 { .. }) {
        4
    } else {
        2
    };
    cpu.execute(&cpu.decode(opcode), width);
}

fn msr(cpu: &mut Processor, register: u32, value: u32) {
    cpu.r0_12[0] = value;
    execute(
        cpu,
        ThumbCode::Thumb32 {
            opcode: 0xf3808800 | register,
        },
    );
}

fn mrs(cpu: &mut Processor, register: u32) -> u32 {
    execute(
        cpu,
        ThumbCode::Thumb32 {
            opcode: 0xf3ef8100 | register,
        },
    );
    cpu.r0_12[1]
}

#[test]
fn basepri_and_basepri_max_use_only_implemented_priority_bits() {
    for bits in 3..=8 {
        let mut cpu = Processor::new();
        cpu.branch_write_pc(0x100);
        cpu.priority_bits = bits;
        let mask = 0xff & (0xff << (8 - bits));
        for operand in 0..=255 {
            let expected = operand & mask;
            msr(&mut cpu, 17, 0xffff_ff00 | operand);
            assert_eq!(
                mrs(&mut cpu, 17),
                expected,
                "bits={bits}, operand={operand:02x}"
            );
            assert_eq!(mrs(&mut cpu, 18), expected);
            for before in [0, 0x40, 0x80, 0xc0] {
                msr(&mut cpu, 17, before);
                msr(&mut cpu, 18, operand);
                let result = if expected != 0 && (before == 0 || expected < before) {
                    expected
                } else {
                    before
                };
                assert_eq!(
                    mrs(&mut cpu, 17),
                    result,
                    "bits={bits}, before={before:02x}, operand={operand:02x}"
                );
            }
        }
    }
}

#[test]
fn unprivileged_cps_cannot_change_interrupt_or_fault_masks() {
    for opcode in [0xb661, 0xb662, 0xb663, 0xb671, 0xb672, 0xb673] {
        let mut cpu = Processor::new();
        cpu.branch_write_pc(0x100);
        let mut vectors = [0; 256];
        vectors[8..12].copy_from_slice(&0x81_u32.to_le_bytes());
        cpu.flash_memory(vectors.len(), &vectors);
        cpu.msp = 0x2001fff0;
        let disabling = opcode & 0x10 != 0;
        msr(&mut cpu, 16, u32::from(!disabling));
        msr(&mut cpu, 19, u32::from(!disabling));
        msr(&mut cpu, 20, 1); // Drop privilege in Thread mode.
        assert!(!cpu.current_mode_is_privileged());
        let before = cpu.get_pc();
        execute(&mut cpu, ThumbCode::Thumb16 { opcode });
        assert_eq!(cpu.get_pc(), before + 2);
        // NMI enters Handler mode even when both masks are set.
        cpu.exception_entry(Exception::NMI, before + 2).unwrap();
        assert_eq!(
            mrs(&mut cpu, 16),
            u32::from(!disabling),
            "PRIMASK {opcode:04x}"
        );
        assert_eq!(
            mrs(&mut cpu, 19),
            u32::from(!disabling),
            "FAULTMASK {opcode:04x}"
        );
        assert_eq!(cpu.cfsr, 0);
    }
}

#[test]
fn privileged_cps_changes_only_the_selected_masks() {
    for selection in 1u16..=3 {
        let mut cpu = Processor::new();
        cpu.branch_write_pc(0x100);
        for disable in [true, false] {
            let opcode = 0xb660 | (u16::from(disable) << 4) | selection;
            execute(&mut cpu, ThumbCode::Thumb16 { opcode });
            assert_eq!(mrs(&mut cpu, 16), u32::from(disable && selection & 2 != 0));
            assert_eq!(mrs(&mut cpu, 19), u32::from(disable && selection & 1 != 0));
            assert_eq!(cpu.cfsr, 0);
        }
    }
}
