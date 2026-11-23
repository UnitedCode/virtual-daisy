#![cfg(any(feature = "armv7m", feature = "armv7em"))]
use zmu_cortex_m::{
    Processor,
    core::{
        register::{BaseReg, Reg},
        reset::Reset,
        thumb::ThumbCode,
    },
    decoder::Decoder,
    executor::Executor,
};

fn fixture() -> Processor {
    let mut bytes = vec![0; 512];
    for (at, word) in [(0, 0x2001fff0u32), (4, 0x101), (12, 0xc1), (24, 0xc1)] {
        bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    let mut cpu = Processor::new();
    cpu.flash_memory(bytes.len(), &bytes);
    cpu.reset().unwrap();
    cpu.shcsr = 1 << 18;
    cpu
}

#[test]
fn invalid_pc_destinations_fault_without_panicking_or_changing_operands() {
    let mut failures = Vec::new();
    for compiled in [false, true] {
        for opcode in [
            0xf04d2f25, 0xf2464f60, 0xea7b1fe0, 0xf47c0f3c, 0xea611ffb, 0xf5c45f20, 0xf20c1f2f,
            0xe9d0cfcb, 0xe85cf0eb, 0xf2c00f01, 0xf3c10f00, 0xf3410f00, 0xf3610f00, 0xf36f0f00,
            0xfab1ff81, 0xfa91ff81, 0xfa91ff91, 0xfa91ffb1, 0xf3ef8f00, 0xea4f0f41, 0xea4f0f51,
            0xea4f0f61, 0xea4f0f71, 0xfa01ff02,
        ] {
            let mut cpu = fixture();
            cpu.r0_12.fill(0x12345678);
            let before = cpu.r0_12;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
                if compiled {
                    cpu.execute_compiled(&instruction, 4);
                } else {
                    cpu.execute(&instruction, 4);
                }
            }));
            if result.is_err() {
                failures.push(opcode);
                continue;
            }
            assert_ne!(cpu.cfsr & (1 << 16), 0, "{opcode:08x}");
            assert_eq!(cpu.r0_12, before, "{opcode:08x}");
        }
    }
    assert!(failures.is_empty(), "host panics for {failures:08x?}");
}

#[test]
fn preload_and_unallocated_memory_hints_preserve_state_without_data_faults() {
    for compiled in [false, true] {
        for opcode in [
            0xf991f123, 0xf911fc80, 0xf99ff123, 0xf91ff123, 0xf911f012, 0xf8b1f123, 0xf9b1f123,
            0xf831f002, 0xf933fc80,
        ] {
            let mut cpu = fixture();
            cpu.r0_12.fill(0xfffffff0);
            cpu.r0_12[2] = 4;
            let registers = cpu.r0_12;
            let sp = cpu.msp;
            let flags = cpu.psr.value;
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_eq!(cpu.cfsr, 0, "{opcode:08x}");
            assert_eq!(cpu.r0_12, registers);
            assert_eq!(cpu.msp, sp);
            assert_eq!(cpu.psr.value, flags);
            assert_eq!(cpu.get_r(Reg::PC), 0x108);
        }
    }
}

#[test]
fn skipped_invalid_destinations_do_not_fault() {
    for compiled in [false, true] {
        let mut cpu = fixture();
        cpu.psr.value |= 1 << 30;
        let it = cpu.decode(ThumbCode::Thumb16 { opcode: 0xbf18 }); // IT NE; Z=1.
        cpu.execute(&it, 2);
        let before = cpu.r0_12;
        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode: 0xf2464f60 });
        if compiled {
            cpu.execute_compiled(&instruction, 4);
        } else {
            cpu.execute(&instruction, 4);
        }
        assert_eq!(cpu.cfsr, 0);
        assert_eq!(cpu.r0_12, before);
        assert_eq!(cpu.get_r(Reg::PC), 0x10a);
    }
}

#[test]
fn legal_pc_writes_and_comparison_aliases_still_execute() {
    for compiled in [false, true] {
        for (opcode, wide, operand, expected_pc) in [
            (0x4687, false, 0x201, 0x204), // MOV PC,R0.
            (0x4487, false, 4, 0x10c),     // ADD PC,R0.
            (0xf8d1f000, true, 0, 0x204),  // LDR.W PC,[R1].
        ] {
            let mut cpu = fixture();
            cpu.r0_12[0] = operand;
            cpu.r0_12[1] = 0x20000000;
            zmu_cortex_m::bus::Bus::write32(&mut cpu, 0x20000000, 0x201).unwrap();
            let instruction = cpu.decode(if wide {
                ThumbCode::Thumb32 { opcode }
            } else {
                ThumbCode::Thumb16 {
                    opcode: opcode as u16,
                }
            });
            let size = if wide { 4 } else { 2 };
            if compiled {
                cpu.execute_compiled(&instruction, size);
            } else {
                cpu.execute(&instruction, size);
            }
            assert_eq!(cpu.cfsr, 0);
            assert_eq!(cpu.get_r(Reg::PC), expected_pc, "{opcode:08x}");
        }
        for (opcode, expected_flags) in [
            (0xf0110f01, 0),
            (0xf0910f01, 0),
            (0xf1110f01, 0),
            (0xf1b10f01, 0x20000000),
        ] {
            let mut cpu = fixture();
            cpu.r0_12[1] = 3;
            let before = cpu.r0_12;
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            if compiled {
                cpu.execute_compiled(&instruction, 4);
            } else {
                cpu.execute(&instruction, 4);
            }
            assert_eq!(cpu.cfsr, 0, "{opcode:08x}");
            assert_eq!(cpu.psr.value & 0xf0000000, expected_flags);
            assert_eq!(cpu.r0_12, before);
        }
    }
}
