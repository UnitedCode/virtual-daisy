#![cfg(feature = "rust-host")]
use virtual_daisy::{board::Board, machine::Machine};

fn elf(code: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 84];
    bytes[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    for (offset, value) in [
        (16, 2 | (40u32 << 16)),
        (28, 52),
        (40, 52 | (32 << 16)),
        (44, 1),
        (52, 1),
        (56, 84),
        (60, 0x08000000),
        (64, 0x08000000),
        (68, code.len() as u32),
        (72, code.len() as u32),
        (76, 5),
        (80, 4),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(code);
    bytes
}
fn program() -> Vec<u8> {
    let mut code = vec![0; 0xa8];
    code[..4].copy_from_slice(&0x20010000u32.to_le_bytes());
    code[4..8].copy_from_slice(&0x08000081u32.to_le_bytes());
    code[0x80..0x88].copy_from_slice(&[3, 0x48, 4, 0x49, 1, 0x60, 0xfe, 0xe7]);
    code[0x90..0x94].copy_from_slice(&0x20000000u32.to_le_bytes());
    code[0x94..0x98].copy_from_slice(&0x12345678u32.to_le_bytes());
    code
}
#[test]
fn functional_and_timed_execute_the_same_arm_load_store_bytes() {
    let image = elf(&program());
    let mut timed = Machine::from_elf(Board::Rev5, &image).unwrap();
    let mut fast = Machine::from_elf_functional(Board::Rev5, &image).unwrap();
    for machine in [&mut timed, &mut fast] {
        machine.run_until(10000, 5000).unwrap();
        assert_eq!(machine.read32(0x20000000).unwrap(), 0x12345678);
    }
    assert_eq!(timed.registers(), fast.registers());
    fast.write32(0x20000004, 0xabc).unwrap();
    assert_eq!(fast.read32(0x20000004).unwrap(), 0xabc);
    assert!(fast.write32(0x08000080, 0).is_err());
    assert!(fast.request_dma(0).is_err());
    assert!(
        fast.run_until(1000000000, 1).is_err(),
        "finite instruction budget must remain effective"
    );
}

#[test]
fn both_runtimes_deliver_invalid_thumb_state_before_target_side_effects() {
    for board in Board::ALL {
        for functional in [false, true] {
            for enabled in [false, true] {
                for odd in [false, true] {
                    let mut code = vec![0u8; 0x144];
                    for (at, value) in [
                        (0, 0x20010000u32),
                        (4, 0x08000081),
                        (12, 0x08000101),
                        (24, 0x08000101),
                        (0xa0, 0x080000c0 | u32::from(odd)),
                        (0xf0, 0x20000100),
                        (0x140, 0x20000100),
                    ] {
                        code[at..at + 4].copy_from_slice(&value.to_le_bytes());
                    }
                    // LDR R0,target; BX R0. Target writes 0x55 if it executes.
                    for (at, instruction) in [
                        (0x80, 0x4807u16),
                        (0x82, 0x4700),
                        (0x84, 0xe7fe),
                        (0xc0, 0x480b),
                        (0xc2, 0x2155),
                        (0xc4, 0x6001),
                        (0xc6, 0xe7fe),
                        // The fault handler writes 0x66, IPSR, stacked PC/xPSR.
                        (0x100, 0x480f),
                        (0x102, 0x2166),
                        (0x104, 0x6001),
                        (0x106, 0xf3ef),
                        (0x108, 0x8105),
                        (0x10a, 0x6041),
                        (0x10c, 0xf3ef),
                        (0x10e, 0x8208),
                        (0x110, 0x6991),
                        (0x112, 0x6081),
                        (0x114, 0x69d1),
                        (0x116, 0x60c1),
                        (0x118, 0xe7fe),
                    ] {
                        code[at..at + 2].copy_from_slice(&instruction.to_le_bytes());
                    }
                    let image = elf(&code);
                    let mut machine = if functional {
                        Machine::from_elf_functional(board, &image)
                    } else {
                        Machine::from_elf(board, &image)
                    }
                    .unwrap();
                    if enabled {
                        machine.write32(0xe000ed24, 1 << 18).unwrap();
                    }
                    machine.run_until(10_000, 5000).unwrap();
                    assert_eq!(
                        machine.read32(0x20000100).unwrap(),
                        if odd { 0x55 } else { 0x66 }
                    );
                    assert_eq!(
                        machine.read32(0xe000ed28).unwrap(),
                        if odd { 0 } else { 1 << 17 }
                    );
                    assert_eq!(
                        machine.read32(0xe000ed2c).unwrap(),
                        if odd || enabled { 0 } else { 1 << 30 }
                    );
                    if !odd {
                        assert_eq!(
                            machine.read32(0x20000104).unwrap(),
                            if enabled { 6 } else { 3 }
                        );
                        assert_eq!(machine.read32(0x20000108).unwrap(), 0x080000c0);
                        assert_eq!(machine.read32(0x2000010c).unwrap() & (1 << 24), 0);
                    }
                }
            }
        }
    }
}
#[test]
fn functional_wfi_and_nvic_deliver_a_real_arm_interrupt() {
    let mut code = program();
    code[64..68].copy_from_slice(&0x08000091u32.to_le_bytes());
    code[0x80..0x84].copy_from_slice(&[0x30, 0xbf, 0xfd, 0xe7]);
    code[0x90..0x9a].copy_from_slice(&[3, 0x48, 1, 0x68, 1, 0x31, 1, 0x60, 0x70, 0x47]);
    code[0xa0..0xa4].copy_from_slice(&0x20000000u32.to_le_bytes());
    let mut machine = Machine::from_elf_functional(Board::Rev5, &elf(&code)).unwrap();
    machine.write32(0xe000e100, 1).unwrap();
    machine.run_until(1000, 1000).unwrap();
    assert_eq!(machine.read32(0x20000000).unwrap(), 0);
    machine.pend_interrupt(0).unwrap();
    machine.run_until(5000, 2000).unwrap();
    assert_eq!(machine.read32(0x20000000).unwrap(), 1);
}
#[test]
fn functional_reset_preserves_dtcm_and_reenters_the_actual_reset_vector() {
    let mut code = program();
    code.resize(0xac, 0);
    code[0x80..0x94].copy_from_slice(&[
        7, 0x48, 1, 0x68, 1, 0x31, 1, 0x60, 1, 0x29, 2, 0xd1, 5, 0x48, 6, 0x49, 1, 0x60, 0xfe, 0xe7,
    ]);
    for (offset, value) in [
        (0xa0, 0x20000000u32),
        (0xa4, 0xe000ed0c),
        (0xa8, 0x05fa0004),
    ] {
        code[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut machine = Machine::from_elf_functional(Board::Rev5, &elf(&code)).unwrap();
    machine.run_until(10000, 5000).unwrap();
    assert_eq!(machine.resets(), 1);
    assert_eq!(machine.read32(0x20000000).unwrap(), 2);
    machine.write32(0xe000ed0c, 0x05fa0004).unwrap();
    machine.step().unwrap();
    machine.run_until(20000, 5000).unwrap();
    assert_eq!(machine.resets(), 2);
    assert_eq!(machine.read32(0x20000000).unwrap(), 3);
}
#[test]
fn functional_timer_and_gpio_use_the_same_existing_peripheral_models() {
    let mut machine = Machine::from_elf_functional(Board::Rev5, &elf(&program())).unwrap();
    machine.write32(0x580244e8, 1).unwrap();
    for (offset, value) in [
        (0x28, 63),
        (0x2c, 9),
        (0x14, 1),
        (0x10, 0),
        (0xc, 1),
        (0, 1),
    ] {
        machine.write32(0x40000000 + offset, value).unwrap();
    }
    machine.run_until(9000, 3000).unwrap();
    assert_eq!(machine.read32(0x40000010).unwrap() & 1, 0);
    machine.run_until(10100, 3000).unwrap();
    assert_eq!(machine.read32(0x40000010).unwrap() & 1, 1);
    machine.write32(0x580244e0, 1).unwrap();
    machine.write32(0x58020000, 0x55555555).unwrap();
    machine.write32(0x58020018, 1).unwrap();
    assert_eq!(machine.read32(0x58020010).unwrap() & 1, 1);
    machine.write32(0x58020018, 1 << 16).unwrap();
    assert_eq!(machine.read32(0x58020010).unwrap() & 1, 0);
}
