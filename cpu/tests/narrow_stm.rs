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

#[test]
fn narrow_stm_executes_every_defined_low_register_list_and_base() {
    let base = 0x20000010;
    let sentinel = 0xa55ac33c;
    let mut checked = 0;
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        for rn in 0..8u32 {
            for mask in 1..=255u32 {
                // If the base is present but is not the first register, its
                // stored value is UNKNOWN. Check defined transfers only.
                if mask & (1 << rn) != 0 && mask.trailing_zeros() < rn {
                    continue;
                }
                let mut before = [0; 8];
                for (i, word) in before.iter_mut().enumerate() {
                    *word = 0x13570000 + i as u32;
                }
                before[rn as usize] = base;
                cpu.r0_12[..8].copy_from_slice(&before);
                cpu.psr.value = 0xa90a0000;
                cpu.branch_write_pc(0x100);
                for i in 0..10 {
                    cpu.write32(base - 4 + i * 4, sentinel).unwrap();
                }
                let opcode = (0xc000 | (rn << 8) | mask) as u16;
                let instruction = cpu.decode(ThumbCode::Thumb16 { opcode });
                if compiled {
                    cpu.execute_compiled(&instruction, 2);
                } else {
                    cpu.execute(&instruction, 2);
                }
                assert_eq!(cpu.cfsr, 0, "{opcode:04x}");
                let mut at = base;
                for (i, &value) in before.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        assert_eq!(cpu.read32(at).unwrap(), value, "{opcode:04x} R{i}");
                        at += 4;
                    }
                }
                assert_eq!(cpu.read32(base - 4).unwrap(), sentinel);
                while at <= base + 32 {
                    assert_eq!(
                        cpu.read32(at).unwrap(),
                        sentinel,
                        "{opcode:04x} guard={at:x}"
                    );
                    at += 4;
                }
                for (i, &value) in before.iter().enumerate() {
                    assert_eq!(
                        cpu.get_r(Reg::from(i as u8)),
                        if i == rn as usize {
                            base + 4 * mask.count_ones()
                        } else {
                            value
                        }
                    );
                }
                assert_eq!(cpu.psr.value, 0xa90a0000);
                assert_eq!(cpu.get_r(Reg::PC), 0x106);
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 2542);
}

#[test]
fn empty_narrow_store_lists_use_the_undefined_encoding_policy() {
    let cpu = Processor::new();
    for rn in 0..8u16 {
        assert!(matches!(
            cpu.decode(ThumbCode::Thumb16 {
                opcode: 0xc000 | (rn << 8)
            }),
            Instruction::UDF { .. }
        ));
    }
}
