//! Adapter regressions against the peripheral behavior used by the Renode host.
use super::*;
use zmu_cortex_m::core::exception::{Exception, ExceptionHandling};

fn interrupt_fixture(n: usize) -> Firmware {
    let mut image = vec![0; 0x440];
    image[..4].copy_from_slice(&0x20010000u32.to_le_bytes());
    image[4..8].copy_from_slice(&0x08000401u32.to_le_bytes());
    let vector = (16 + n) * 4;
    image[vector..vector + 4].copy_from_slice(&0x08000421u32.to_le_bytes());
    image[0x400..0x402].copy_from_slice(&0xe7feu16.to_le_bytes()); // b .
    image[0x420..0x424].copy_from_slice(&[0x5a, 0x24, 0xfe, 0xe7]); // movs r4,#90; b .
    Firmware::new(&image).unwrap()
}

fn assert_delivered(f: &mut Firmware, n: usize) {
    let bank = (n / 32) as u32 * 4;
    let mask = 1 << (n % 32);
    f.cpu.write32(0xe000e100 + bank, mask).unwrap();
    f.service().unwrap();
    assert!(
        f.cpu.exception_pending(Exception::Interrupt { n }),
        "IRQ{n} never reached NVIC"
    );
    for _ in 0..5 {
        f.cpu.step();
    }
    assert_eq!(f.cpu.get_r(Reg::R4), 90, "IRQ{n} handler did not execute");
    assert!(f.cpu.exception_active(Exception::Interrupt { n }));
    assert_eq!(f.cpu.read32(0xe000e200 + bank).unwrap() & mask, 0);
}

#[test]
fn sai_fifo_interrupt_reaches_the_arm_handler() {
    let mut f = interrupt_fixture(87);
    // RM0433 SAI_xSR.FREQ is set for an empty TX FIFO even before SAIEN.
    assert_eq!(f.board.borrow_mut().read(0x40015818, 4).unwrap() & 8, 8);
    f.service().unwrap();
    assert!(!f.cpu.exception_pending(Exception::Interrupt { n: 87 }));
    f.board.borrow_mut().write(0x40015814, 4, 8).unwrap();
    assert_delivered(&mut f, 87);
}

#[test]
fn qspi_transfer_complete_interrupt_reaches_the_arm_handler() {
    let mut f = interrupt_fixture(92);
    {
        let mut board = f.board.borrow_mut();
        board.write(0x52005000, 4, 1).unwrap(); // EN; TCIE initially masked
        board.write(0x52005014, 4, 0x106).unwrap(); // SPI WREN, no data
        assert_eq!(board.read(0x52005008, 4).unwrap() & 2, 2);
    }
    f.service().unwrap();
    assert!(!f.cpu.exception_pending(Exception::Interrupt { n: 92 }));
    f.board
        .borrow_mut()
        .write(0x52005000, 4, 1 | (1 << 17))
        .unwrap();
    assert_delivered(&mut f, 92);
}

#[test]
fn a_new_sai_edge_is_latched_while_the_handler_is_active() {
    let mut f = interrupt_fixture(87);
    f.board.borrow_mut().write(0x40015814, 4, 8).unwrap();
    assert_delivered(&mut f, 87);
    f.service().unwrap();
    assert!(
        !f.cpu.exception_pending(Exception::Interrupt { n: 87 }),
        "steady active level must not repend early"
    );
    f.board.borrow_mut().write(0x40015814, 4, 0).unwrap();
    f.service().unwrap();
    f.board.borrow_mut().write(0x40015814, 4, 8).unwrap();
    f.service().unwrap();
    assert!(
        f.cpu.exception_pending(Exception::Interrupt { n: 87 }),
        "a rising edge must pend even while active"
    );
}

#[test]
fn disabled_nvic_latches_a_dma_interrupt_until_enabled() {
    let mut f = interrupt_fixture(11);
    {
        let mut b = f.board.borrow_mut();
        b.write(0x40020014, 4, 4).unwrap();
        b.write(0x40020018, 4, 0x40015820).unwrap();
        b.write(0x4002001c, 4, 0x30000000).unwrap();
        b.write(0x40020020, 4, 0x30000100).unwrap();
        b.write(0x40020010, 4, 1 | 4 | (2 << 11) | (2 << 13) | (1 << 18))
            .unwrap();
        // RM0433: rewriting an active DBM address sets TEIF and clears EN.
        b.write(0x4002001c, 4, 0x30000200).unwrap();
        assert_eq!(b.read(0x40020000, 4).unwrap() & 8, 8);
    }
    f.service().unwrap();
    assert_eq!(f.cpu.read32(0xe000e200).unwrap() & (1 << 11), 1 << 11);
    assert!(
        f.cpu.get_pending_exception().is_none(),
        "disabled IRQ was eligible"
    );
    f.cpu.step();
    assert_eq!(f.cpu.get_r(Reg::R4), 0, "disabled IRQ executed its handler");
    // The device can deassert after NVIC has latched it; enabling must still deliver.
    f.board.borrow_mut().write(0x40020008, 4, 8).unwrap();
    assert_delivered(&mut f, 11);
}
