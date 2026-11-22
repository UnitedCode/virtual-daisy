use virtual_daisy::uart::Transmitter;

#[test]
fn midi_fifo_applies_backpressure_until_a_complete_8n1_frame_leaves() {
    let mut uart = Transmitter::new(16);
    uart.configure(120_000_000, 3840).unwrap(); // 31,250 baud, 320 us/byte.
    for byte in 0..17 {
        uart.write(byte, 0).unwrap(); // One shifter plus sixteen FIFO slots.
    }
    assert!(!uart.ready(319_999));
    assert!(uart.write(99, 319_999).is_err());
    assert!(uart.ready(320_000));
    assert_eq!(uart.take_completed(), vec![(320_000, 0)]);
    uart.write(99, 320_000).unwrap();
    assert!(!uart.ready(639_999));
    assert!(uart.ready(640_000));
    assert_eq!(uart.take_completed(), vec![(640_000, 1)]);
}

#[test]
fn transmission_remains_paced_after_an_hour_without_timer_wrap() {
    let mut uart = Transmitter::new(16);
    assert!(uart.write(1, 0).is_err());
    assert!(uart.configure(0, 3840).is_err());
    assert!(uart.configure(120_000_000, 15).is_err());
    uart.configure(120_000_000, 3840).unwrap();
    let hour = 3_661_000_000_000;
    uart.write(0x90, hour).unwrap();
    uart.write(60, hour).unwrap();
    assert!(!uart.complete(hour + 639_999));
    assert_eq!(uart.take_completed(), vec![(hour + 320_000, 0x90)]);
    assert!(uart.complete(hour + 640_000));
    assert_eq!(uart.take_completed(), vec![(hour + 640_000, 60)]);
    // This tests the peripheral's time representation, not a firmware soak.
    uart.write(100, hour + 1_000_000).unwrap();
    assert!(!uart.complete(hour + 1_319_999));
    assert!(uart.complete(hour + 1_320_000));
}
