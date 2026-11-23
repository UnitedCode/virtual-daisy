use virtual_daisy::clock::ClockTree;

fn fractional_audio_clock() -> ClockTree {
    let mut clock = ClockTree::default();
    clock.write(0, 1 | (1 << 16));
    clock.write(0x28, 2 | (4 << 20)); // HSE16 / M3=4.
    clock.write(0x40, 99 | (7 << 9)); // N3=100, P3=8.
    clock.write(0x44, 4096 << 3); // +0.5.
    clock.write(0x2c, (1 << 8) | (1 << 22));
    clock.write(0, 1 | (1 << 16) | (1 << 28));
    clock.write(0x50, 2); // PLL3P.
    clock.write(0xf0, 1 << 22); // APB2 SAI1 clock.
    clock
}

#[test]
fn audio_observations_keep_exact_first_edge_across_split_intervals_and_stops() {
    let mut whole = fractional_audio_clock();
    let mut split = fractional_audio_clock();
    let cr = (1 << 16) | (4 << 20);
    for clock in [&mut whole, &mut split] {
        clock.advance(0, 63, 9_000).unwrap();
    }
    let next = whole.next_audio_ns(cr, 63).unwrap().unwrap();
    whole.advance(cr, 63, 100_000).unwrap();
    split.advance(cr, 63, next - 1).unwrap();
    assert_eq!(split.audio_observations().first_frame_ns, None);
    split.advance(cr, 63, 1).unwrap();
    assert_eq!(
        split.audio_observations().first_frame_ns,
        Some(u128::from(9_000 + next))
    );
    split.advance(cr, 63, 100_000 - next).unwrap();
    assert_eq!(whole.audio_observations(), split.audio_observations());
    let observed = whole.audio_observations();
    assert_eq!(observed.first_running_interval_ns, Some(9_000));
    assert_eq!(observed.elapsed_ns, 109_000);
    assert_eq!(observed.generated_frames, 4);
    whole.advance(0, 63, 1_000).unwrap();
    assert_eq!(
        whole.audio_observations().first_frame_ns,
        observed.first_frame_ns
    );
    assert_eq!(whole.audio_observations().generated_frames, 4);
    assert!(whole
        .stats(cr, 63)
        .unwrap()
        .contains("sai_clock_timestamp_scope=MODEL_SINCE_RESET"));
}

#[test]
fn stopped_and_zero_length_intervals_do_not_invent_audio_observations() {
    let mut clock = ClockTree::default();
    clock.advance(0, 0, 10_000).unwrap();
    assert_eq!(clock.audio_observations().first_running_interval_ns, None);
    assert_eq!(clock.audio_observations().first_frame_ns, None);
    assert!(clock
        .audio_observations()
        .report()
        .contains("sai_clock_first_frame_ns=NOT_OBSERVED"));
    let mut enabled = fractional_audio_clock();
    let cr = (1 << 16) | (4 << 20);
    enabled.advance(cr, 63, 0).unwrap();
    assert_eq!(enabled.audio_observations(), Default::default());
}

#[test]
fn audio_deadlines_match_exact_frame_edges_across_fractional_phases_and_gates() {
    let mut clock = fractional_audio_clock();
    let cr = (1 << 16) | (4 << 20);
    for n in 0..300 {
        let cr = (1 << 16) | ((if n < 150 { 4 } else { 3 }) << 20);
        let next = clock.next_audio_ns(cr, 63).unwrap().unwrap();
        assert_eq!(clock.advance(cr, 63, next - 1).unwrap(), 0);
        assert_eq!(clock.next_audio_ns(cr, 63).unwrap(), Some(1));
        assert_eq!(clock.advance(cr, 63, 1).unwrap(), 1);
    }
    assert_eq!(clock.next_audio_ns(0, 63).unwrap(), None);
    clock.write(0xf0, 0);
    assert_eq!(clock.next_audio_ns(cr, 63).unwrap(), None);
    clock.write(0xf0, 1 << 22);
    clock.write(0x50, 4);
    clock.write(0x4c, 2 << 28);
    clock.write(0, 1);
    assert_eq!(clock.next_audio_ns(cr, 63).unwrap(), None);
}

#[test]
fn cached_clock_advance_matches_wide_arithmetic_and_configuration_changes() {
    let mut clock = fractional_audio_clock();
    let mut phase = 0u128;
    let mut last = None;
    for n in 0..2000 {
        if n == 700 {
            clock.write(0x50, 0);
        }
        if n == 1100 {
            clock.write(0x50, 2);
        }
        let cr1 = (1 << 16) | ((if n < 1400 { 4 } else { 3 }) << 20);
        let frcr = if n < 1600 { 63 } else { 127 };
        let rate = clock.sample_rate(cr1, frcr).unwrap();
        if last != Some(rate) {
            phase = 0;
            last = Some(rate);
        }
        let ns = if n == 1700 {
            9_000_000_000
        } else {
            997 + (n % 13) as u64
        };
        let denominator = u128::from(rate.denominator) * 1_000_000_000;
        phase += u128::from(rate.numerator) * u128::from(ns);
        let expected = phase / denominator;
        phase %= denominator;
        assert_eq!(u128::from(clock.advance(cr1, frcr, ns).unwrap()), expected);
    }
}

#[test]
fn pll_fraction_and_sai_dividers_drive_frame_count_without_integer_hz_drift() {
    let mut clock = fractional_audio_clock();
    let cr1 = (1 << 16) | (4 << 20);
    assert_eq!(clock.sample_rate(cr1, 63).unwrap().hz(), 49072.265625);
    let mut frames = 0;
    for _ in 0..400_000 {
        frames += clock.advance(cr1, 63, 10_000).unwrap();
    }
    assert_eq!(frames, 196289); // floor(4 s * 49,072.265625 Hz).
}

#[test]
fn stopped_pll_or_gated_sai_cannot_produce_frames() {
    let mut clock = fractional_audio_clock();
    clock.write(0, 1 | (1 << 16));
    assert_eq!(clock.advance(1 << 16, 63, 1_000_000).unwrap(), 0);
    clock.write(0, 1 | (1 << 16) | (1 << 28));
    assert_eq!(clock.advance(0, 63, 1_000_000).unwrap(), 0);
    clock.write(0xf0, 0);
    assert_eq!(clock.advance(1 << 16, 63, 1_000_000).unwrap(), 0);
}

#[test]
fn dwt_cycle_counter_uses_core_clock_changes_and_wraps_at_32_bits() {
    let mut clock = ClockTree::default();
    clock.dwt(0xfb0, Some(0xc5acce55), 0).unwrap();
    clock.trace_enable(true, 0).unwrap();
    clock.dwt(4, 0xfffffff0_u32.into(), 0).unwrap();
    clock.dwt(0, Some(1), 0).unwrap(); // Reset HSI64MHz.
    assert_eq!(clock.dwt(4, None, 1000).unwrap(), 48);
    clock.sync_cycles(1000).unwrap();
    clock.write(0, 1 | (1 << 16));
    clock.write(0x10, 2); // HSE16.
    assert_eq!(clock.dwt(4, None, 2000).unwrap(), 64);
    clock.dwt(0, Some(0), 2000).unwrap();
    assert_eq!(clock.dwt(4, None, 3000).unwrap(), 64);
}

#[test]
fn dwt_software_lock_requires_the_coresight_key() {
    let mut clock = ClockTree::default();
    clock.trace_enable(true, 0).unwrap();
    assert_eq!(clock.dwt(0xfb4, None, 0).unwrap(), 3);
    clock.dwt(0, Some(1), 0).unwrap();
    assert_eq!(clock.dwt(4, None, 1000).unwrap(), 0);
    clock.dwt(0xfb0, Some(0xc5acce55), 1000).unwrap();
    assert_eq!(clock.dwt(0xfb4, None, 1000).unwrap(), 1);
    clock.dwt(0, Some(1), 1000).unwrap();
    assert_eq!(clock.dwt(4, None, 2000).unwrap(), 64);
    clock.dwt(0xfb0, Some(0), 2000).unwrap();
    clock.dwt(4, Some(0), 2000).unwrap();
    assert_eq!(clock.dwt(4, None, 2000).unwrap(), 64);
}

#[test]
fn dwt_trace_disable_stops_cycles_at_the_demcr_write_time() {
    let mut clock = ClockTree::default();
    clock.dwt(0xfb0, Some(0xc5acce55), 0).unwrap();
    clock.dwt(0, Some(1), 0).unwrap();
    assert_eq!(clock.dwt(4, None, 1000).unwrap(), 0);
    clock.trace_enable(true, 1000).unwrap();
    clock.trace_enable(false, 2000).unwrap();
    assert_eq!(clock.dwt(4, None, 4000).unwrap(), 64);
}

#[test]
fn production_pll_programming_exposes_the_firmware_sample_rate_difference() {
    let mut clock = ClockTree::default();
    clock.write(0, 1 | (1 << 16));
    clock.write(0x28, 0x00308012);
    clock.write(0x40, 0x4c4c98b1);
    clock.write(0x44, 0x19d8);
    clock.write(0x2c, 0x01cf092c);
    clock.write(0, 0x3f0331a5);
    clock.write(0x50, 2);
    let rate = clock.sample_rate(0x001302c1, 0x00011f3f).unwrap();
    // Worked RM0433 case: HSE16MHz / 3 * (178 + 827/8192) / 77 / 256.
    assert_eq!(rate.numerator, 3_256_703_125);
    assert_eq!(rate.denominator, 67_584);
    assert!((rate.hz() - 48_187.487053).abs() < 0.000001);
}

#[test]
fn apb_timer_multiplier_and_nonuniform_high_prescalers_follow_the_clock_tree() {
    let mut clock = ClockTree::default(); // HSI64.
    clock.write(0x18, 8); // HCLK /2.
    clock.write(0x1c, 5 << 4); // APB1 /4.
    assert_eq!(clock.domain_rate(1).unwrap().unwrap().hz(), 32_000_000.0);
    assert_eq!(clock.domain_rate(2).unwrap().unwrap().hz(), 8_000_000.0);
    assert_eq!(clock.domain_rate(6).unwrap().unwrap().hz(), 16_000_000.0);
    clock.write(0x10, 1 << 15); // TIMPRE=1: HCLK for APB /1,/2,/4.
    assert_eq!(clock.domain_rate(6).unwrap().unwrap().hz(), 32_000_000.0);
    clock.write(0x18, 12 << 8); // D1CPRE code12 means /64, not /32.
    assert_eq!(clock.core_rate().unwrap().unwrap().hz(), 1_000_000.0);
}

#[test]
fn ckper_sai_clock_stops_when_its_oscillator_is_disabled() {
    let mut clock = ClockTree::default();
    clock.write(0x50, 4);
    clock.write(0x4c, 2 << 28); // HSE not enabled yet.
    assert_eq!(clock.sample_rate(0, 63).unwrap().hz(), 0.0);
    clock.write(0, 1 | (1 << 16));
    assert_eq!(clock.sample_rate(0, 63).unwrap().hz(), 62500.0);
}

#[test]
fn fractional_changes_latch_only_after_fracen_rising_edge() {
    let mut clock = fractional_audio_clock();
    clock.write(0x44, 0); // Locked while FRACEN is set.
    assert_eq!(clock.sample_rate(4 << 20, 63).unwrap().hz(), 49072.265625);
    clock.write(0x2c, 1 << 22);
    assert_eq!(clock.sample_rate(4 << 20, 63).unwrap().hz(), 49072.265625);
    clock.write(0x44, 0);
    clock.write(0x2c, (1 << 22) | (1 << 8));
    assert_eq!(clock.sample_rate(4 << 20, 63).unwrap().hz(), 48828.125);
}

#[test]
fn external_memory_can_use_ckper_and_obeys_oscillator_gates() {
    let mut clock = ClockTree::default();
    clock.write(0x4c, (2 << 28) | 3); // FMCSEL=CLKP, CKPERSEL=HSE.
    assert_eq!(clock.fmc_rate().unwrap(), None);
    clock.write(0, 1 | (1 << 16));
    assert_eq!(clock.fmc_rate().unwrap().unwrap().hz(), 16_000_000.0);
    clock.write(0x4c, 3); // CKPERSEL=HSI.
    assert_eq!(clock.fmc_rate().unwrap().unwrap().hz(), 64_000_000.0);
    clock.write(0x10, 2); // Run from HSE before disabling HSI.
    clock.write(0, 1 << 16);
    assert_eq!(clock.fmc_rate().unwrap(), None);
}

#[test]
fn qspi_kernel_mux_selects_hclk_pll1q_pll2r_or_ckper() {
    let mut clock = ClockTree::default();
    clock.write(0x28, (1 << 4) | (2 << 12)); // HSI, M1=1, M2=2.
    clock.write(0x30, 63 | (31 << 16)); // PLL1 Q: 64MHz * 64 / 32.
    clock.write(0x38, 29 | (9 << 24)); // PLL2 R: 64MHz / 2 * 30 / 10.
    clock.write(0x2c, (1 << 17) | (1 << 21));
    clock.write(0, 1 | (1 << 16) | (1 << 24) | (1 << 26));
    for (selector, expected) in [64_000_000.0, 128_000_000.0, 96_000_000.0, 16_000_000.0]
        .into_iter()
        .enumerate()
    {
        clock.write(0x4c, ((selector as u32) << 4) | (2 << 28));
        assert_eq!(clock.qspi_rate().unwrap().unwrap().hz(), expected);
    }
}
