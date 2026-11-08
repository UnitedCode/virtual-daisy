use virtual_daisy::keyboard::{validate_log, Carrier};

#[test]
fn paired_muxes_select_the_requested_sensor_without_mixing_outputs() {
    let mut board = Carrier::default();
    board.set_input(9, 4, 2_200_000).unwrap(); // AM10 X4 = HE71.
    board.set_input(10, 4, 1_100_000).unwrap(); // AM11 X4 = HE74.
    assert_eq!(board.sample(17, 1 << 4, 0, 0, 0).unwrap(), 2_200_000);
    assert_eq!(board.sample(17, 1 << 4, 1 << 8, 0, 0).unwrap(), 1_100_000);
    assert!(board.sample(17, 1 << 4, 1 << 9, 0, 0).is_err());
}

#[test]
fn scans_are_gpio_selected_sweeps_and_not_calibration_reads() {
    let mut board = Carrier::default();
    for t in 0..64 {
        board.sample(10, 0, 0, 0, t).unwrap();
    }
    assert_eq!(board.scans, 0);
    for t in [1_000_000, 2_000_000, 12_200_000] {
        board.sample(10, 1 << 4, 0, 3 << 10, t - 1).unwrap();
        board.sample(10, 0, 0, 0, t).unwrap();
    }
    assert_eq!(board.scans, 3);
    assert_eq!(board.max_gap_ns, 10_200_000);
    assert_eq!(board.gaps_over_2ms, 1);
    assert!(board.set_input(15, 0, 0).is_err());
    assert!(board.set_input(0, 8, 0).is_err());
    assert!(board.set_input(0, 0, 3_300_001).is_err());
    assert_eq!(board.sample(14, 0, 5 << 8, 0, 0).unwrap(), 0);
}

#[test]
fn unknown_runtime_warning_or_missing_byte_write_invalidates_the_run() {
    assert!(validate_log("[WARNING] usart1: Byte write to DoubleWord peripheral ignored").is_err());
    assert!(validate_log("[ERROR] cpu: fault").is_err());
    assert!(validate_log("[WARNING] usart1: Unhandled write to offset 0x2C, value 0x0.").is_err());
    assert!(validate_log("[WARNING] [no-name]: Unhandled write to offset 0xC. Unhandled bits: [16] when writing value 0x10008. Tags: DISCEN (0x1). (123)").is_ok());
}
