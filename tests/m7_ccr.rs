#![cfg(feature = "rust-host")]
use virtual_daisy::{board::Board, machine::Machine};

fn firmware() -> Vec<u8> {
    let mut code = vec![0; 0xbc];
    code[..4].copy_from_slice(&0x2001_0004u32.to_le_bytes());
    code[4..8].copy_from_slice(&0x0800_0081u32.to_le_bytes());
    code[44..48].copy_from_slice(&0x0800_00a1u32.to_le_bytes());
    // SVC; save restored MSP; loop. Handler saves frame SP and stacked xPSR.
    for (start, instructions) in [
        (
            0x80,
            &[0xdf00u16, 0xf3ef, 0x8308, 0x4805, 0x6083, 0xe7fe][..],
        ),
        (
            0xa0,
            &[0xf3ef, 0x8108, 0x4804, 0x6001, 0x69ca, 0x6042, 0x4770][..],
        ),
    ] {
        for (i, instruction) in instructions.iter().enumerate() {
            code[start + i * 2..start + i * 2 + 2].copy_from_slice(&instruction.to_le_bytes());
        }
    }
    for at in [0x9c, 0xb8] {
        code[at..at + 4].copy_from_slice(&0x2000_0000u32.to_le_bytes());
    }
    code
}

#[test]
fn m7_ccr_keeps_read_only_bits_and_masks_reserved_bits() {
    for board in Board::ALL {
        let mut machine = Machine::from_binary(board, &firmware()).unwrap();
        assert_eq!(machine.read32(0xe000_ed14).unwrap(), 0x0004_0200);
        for (written, expected) in [
            (0, 0x0004_0200),
            (u32::MAX, 0x0007_031b),
            (0x0003_011b, 0x0007_031b),
            (!0x0003_011b, 0x0004_0200),
        ] {
            machine.write32(0xe000_ed14, written).unwrap();
            assert_eq!(machine.read32(0xe000_ed14).unwrap(), expected);
        }
        machine.write32(0xe000_ed0c, 0x05fa_0004).unwrap();
        machine.step().unwrap();
        assert_eq!(machine.resets(), 1);
        assert_eq!(machine.read32(0xe000_ed14).unwrap(), 0x0004_0200);
    }
}

#[test]
fn clearing_stkalign_cannot_disable_m7_exception_frame_alignment() {
    let mut machine = Machine::from_binary(Board::Rev5, &firmware()).unwrap();
    machine.write32(0xe000_ed14, 0).unwrap();
    machine.run_until(10_000, 5000).unwrap();
    assert_eq!(machine.read32(0x2000_0000).unwrap(), 0x2000_ffe0);
    assert_ne!(machine.read32(0x2000_0004).unwrap() & (1 << 9), 0);
    assert_eq!(machine.read32(0x2000_0008).unwrap(), 0x2001_0004);
    assert_eq!(machine.read32(0xe000_ed28).unwrap(), 0);
}
