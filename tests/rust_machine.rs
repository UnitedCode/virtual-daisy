#![cfg(feature = "rust-host")]
use virtual_daisy::{
    board::Board,
    machine::{InstructionPacing, Machine},
};

fn firmware() -> Vec<u8> {
    let mut code = vec![0; 0x98];
    code[..4].copy_from_slice(&0x20010000u32.to_le_bytes());
    code[4..8].copy_from_slice(&0x08000081u32.to_le_bytes());
    code[0x80..0x88].copy_from_slice(&[3, 0x48, 4, 0x49, 1, 0x60, 0xfe, 0xe7]);
    code[0x90..0x94].copy_from_slice(&0x20000000u32.to_le_bytes());
    code[0x94..0x98].copy_from_slice(&0x12345678u32.to_le_bytes());
    code
}
#[test]
fn actual_arm_loads_and_stores_complete_through_the_shared_memory_path() {
    let mut m = Machine::from_binary(Board::Rev5, &firmware()).unwrap();
    m.run_until(10_000, 2000).unwrap();
    assert_eq!(m.read32(0x20000000).unwrap(), 0x12345678);
}

#[test]
fn one_cycle_pacing_changes_elapsed_time_and_preserves_register_results() {
    let image = firmware();
    let mut modeled = Machine::from_binary(Board::Rev5, &image).unwrap();
    let mut one_cycle = Machine::from_binary(Board::Rev5, &image).unwrap();
    one_cycle
        .set_instruction_pacing(InstructionPacing::OneCycle)
        .unwrap();
    for _ in 0..500 {
        modeled.step().unwrap();
        one_cycle.step().unwrap();
    }
    assert_eq!(modeled.registers(), one_cycle.registers());
    assert_eq!(one_cycle.read32(0x20000000).unwrap(), 0x12345678);
    assert!(one_cycle.time_ns() < modeled.time_ns());

    let modeled_start = modeled.time_ns();
    let one_cycle_start = one_cycle.time_ns();
    for machine in [&mut modeled, &mut one_cycle] {
        machine.write32(0xe000ed0c, 0x05fa0004).unwrap();
        for _ in 0..500 {
            machine.step().unwrap();
        }
        assert_eq!(machine.resets(), 1);
    }
    assert_eq!(modeled.registers(), one_cycle.registers());
    assert!(one_cycle.time_ns() - one_cycle_start < modeled.time_ns() - modeled_start);
}

#[test]
fn floating_transfers_wrap_into_itcm_without_faults_or_data_changes() {
    let mut code = vec![0; 0xa8];
    code[..4].copy_from_slice(&0x20010000u32.to_le_bytes());
    code[4..8].copy_from_slice(&0x08000081u32.to_le_bytes());
    // R1=0xfffffffc. VLDR/VSTR D7 and S18 wrap their positive offsets into
    // ITCM at 0x40, 0x48 and 0x50; the final store marks completion.
    for (i, instruction) in [
        0x4907u16, 0xed91, 0x7b11, 0xed81, 0x7b15, 0xed91, 0x9a11, 0xed81, 0x9a13, 0x225a, 0x4803,
        0x6002, 0xe7fe,
    ]
    .iter()
    .enumerate()
    {
        code[0x80 + i * 2..0x82 + i * 2].copy_from_slice(&instruction.to_le_bytes());
    }
    code[0xa0..0xa4].copy_from_slice(&0xfffffffcu32.to_le_bytes());
    code[0xa4..0xa8].copy_from_slice(&0x20000100u32.to_le_bytes());
    let mut machine = Machine::from_binary(Board::Rev5, &code).unwrap();
    machine.write32(0xe000ed88, 0x00f00000).unwrap();
    machine.write32(0x40, 0x11223344).unwrap();
    machine.write32(0x44, 0x55667788).unwrap();
    machine.run_until(10_000, 2000).unwrap();
    assert_eq!(machine.read32(0x20000100).unwrap(), 0x5a);
    assert_eq!(machine.read32(0x48).unwrap(), 0x11223344);
    assert_eq!(machine.read32(0x50).unwrap(), 0x11223344);
    assert_eq!(machine.read32(0x54).unwrap(), 0x55667788);
    assert_eq!(machine.read32(0xe000ed28).unwrap(), 0);
}
#[test]
fn board_profiles_present_the_physical_revision_straps() {
    for board in Board::ALL {
        let mut m = Machine::from_binary(board, &firmware()).unwrap();
        m.write32(0x580244e0, 1 << 3).unwrap();
        m.write32(0x58020c00, !((3 << 6) | (3 << 8) | (3 << 10)))
            .unwrap();
        m.write32(0x58020c0c, (1 << 6) | (1 << 8) | (1 << 10))
            .unwrap();
        assert_eq!(
            m.read32(0x58020c10).unwrap() & 0x38,
            0x38 & !(board.grounded_straps() << 3)
        );
    }
}
#[test]
fn malformed_and_foreign_elfs_are_rejected_before_execution() {
    for bytes in [vec![], b"not an ELF".to_vec(), vec![0; 64]] {
        assert!(Machine::from_elf(Board::Rev5, &bytes).is_err());
    }
}

#[test]
fn clocks_resets_and_timer_update_are_visible_through_real_mmio() {
    let mut m = Machine::from_binary(Board::Rev5, &firmware()).unwrap();
    m.write32(0x4000002c, 9).unwrap(); // Gate off: ignored.
    assert_eq!(m.read32(0x4000002c).unwrap(), 0);
    m.write32(0x580244e8, 1).unwrap();
    assert_eq!(m.read32(0x4000002c).unwrap(), u32::MAX);
    for (offset, value) in [
        (0x28, 63),
        (0x2c, 9),
        (0x14, 1),
        (0x10, 0),
        (0xc, 1),
        (0, 1),
    ] {
        m.write32(0x40000000 + offset, value).unwrap();
    }
    m.run_until(9000, 3000).unwrap();
    assert_eq!(m.read32(0x40000010).unwrap() & 1, 0);
    m.run_until(10_100, 3000).unwrap();
    assert_eq!(m.read32(0x40000010).unwrap() & 1, 1);
    assert_ne!(m.read32(0xe000e200).unwrap() & (1 << 28), 0);
    m.write32(0x58024490, 1).unwrap();
    m.write32(0x58024490, 0).unwrap();
    assert_eq!(m.read32(0x4000002c).unwrap(), u32::MAX);
    assert_eq!(m.read32(0x40000000).unwrap(), 0);
}

#[test]
fn rcc_enable_aliases_share_clock_gates_and_reset_values() {
    const RCC: u32 = 0x58024400;
    const APB1LENR: u32 = RCC + 0xe8;
    const APB4ENR: u32 = RCC + 0xf4;
    const C1_ALIAS: u32 = 0x60;
    const RTCAPBEN: u32 = 1 << 16;
    const TIM2_ARR: u32 = 0x4000002c;
    let mut m = Machine::from_binary(Board::Rev5, &firmware()).unwrap();
    assert_eq!(m.read32(APB4ENR).unwrap(), RTCAPBEN);
    assert_eq!(m.read32(APB4ENR + C1_ALIAS).unwrap(), RTCAPBEN);
    assert_eq!(m.read32(TIM2_ARR).unwrap(), 0);

    m.write32(APB1LENR + C1_ALIAS, 1).unwrap();
    assert_eq!(m.read32(APB1LENR).unwrap(), 1);
    assert_eq!(m.read32(TIM2_ARR).unwrap(), u32::MAX);
    m.write32(APB1LENR, 0).unwrap();
    assert_eq!(m.read32(APB1LENR + C1_ALIAS).unwrap(), 0);
    assert_eq!(m.read32(TIM2_ARR).unwrap(), 0);

    m.write32(APB4ENR + C1_ALIAS, 0).unwrap();
    assert_eq!(m.read32(APB4ENR).unwrap(), 0);
    m.write32(0xe000ed0c, 0x05fa0004).unwrap(); // SYSRESETREQ
    m.step().unwrap();
    assert_eq!(m.resets(), 1);
    assert_eq!(m.read32(APB4ENR).unwrap(), RTCAPBEN);
    assert_eq!(m.read32(APB4ENR + C1_ALIAS).unwrap(), RTCAPBEN);
}

#[test]
fn serial_frames_are_baud_paced_and_rx_overrun_does_not_invent_bytes() {
    let mut m = Machine::from_binary(Board::Rev5, &firmware()).unwrap();
    m.write32(0x580244f0, 1 << 4).unwrap();
    m.write32(0x4001100c, 2048).unwrap(); // 64 MHz / 2048 = 31,250 baud.
    m.write32(0x40011000, 13 | (1 << 5)).unwrap();
    m.write32(0x40011028, 0x90).unwrap();
    m.run_until(319_000, 100_000).unwrap();
    assert!(m.take_serial().is_empty());
    m.run_until(321_000, 100_000).unwrap();
    assert_eq!(m.take_serial(), vec![(320_000, 0x90)]);
    m.receive_serial(&[0x12, 0x34]).unwrap();
    m.run_until(962_000, 100_000).unwrap();
    assert_eq!(m.read32(0x4001101c).unwrap() & 0x28, 0x28);
    assert_eq!(m.read32(0x40011024).unwrap(), 0x12);
    m.write32(0x40011020, 8).unwrap();
    assert_eq!(m.read32(0x4001101c).unwrap() & 0x28, 0);
}

#[test]
fn actual_arm_byte_and_halfword_uart_accesses_consume_and_transmit_one_character() {
    let mut code = vec![0; 0xac];
    code[..4].copy_from_slice(&0x20010000_u32.to_le_bytes());
    code[4..8].copy_from_slice(&0x08000081_u32.to_le_bytes());
    // Wait for the test's ready flag, then LDRB/LDRH RDR and STRB/STRH TDR.
    for (i, instruction) in [
        0x4d08_u16, 0x68ee, 0x2e00, 0xd0fc, 0x4805, 0x7801, 0x6029, 0x8802, 0x606a, 0x4b05, 0x7019,
        0x801a, 0x245a, 0x60ac, 0xe7fe,
    ]
    .iter()
    .enumerate()
    {
        code[0x80 + i * 2..0x82 + i * 2].copy_from_slice(&instruction.to_le_bytes());
    }
    for (at, value) in [
        (0xa0, 0x40011024_u32),
        (0xa4, 0x20000000),
        (0xa8, 0x40011028),
    ] {
        code[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut m = Machine::from_binary(Board::Rev5, &code).unwrap();
    m.write32(0x580244f0, 1 << 4).unwrap();
    m.write32(0x4001100c, 2048).unwrap();
    m.write32(0x40011000, 13 | (1 << 29)).unwrap();
    m.receive_serial(&[0x90, 0x45]).unwrap();
    m.run_until(641_000, 200_000).unwrap();
    m.write32(0x2000000c, 1).unwrap();
    m.run_until(1_300_000, 200_000).unwrap();
    assert_eq!(m.read32(0x20000000).unwrap(), 0x90);
    assert_eq!(m.read32(0x20000004).unwrap(), 0x45);
    assert_eq!(m.read32(0x20000008).unwrap(), 0x5a);
    let serial = m.take_serial();
    assert_eq!(
        serial.iter().map(|(_, b)| *b).collect::<Vec<_>>(),
        [0x90, 0x45]
    );
    assert_eq!(serial[1].0 - serial[0].0, 320_000);
    assert_eq!(m.read32(0x4001101c).unwrap() & 0x28, 0);
    assert_eq!(m.read32(0xe000ed28).unwrap(), 0);
}

#[test]
fn systick_uses_the_selected_reference_clock_and_clear_on_read_flag() {
    for (source, flag) in [(0, 0), (4, 1 << 16)] {
        let mut m = Machine::from_binary(Board::Rev5, &firmware()).unwrap();
        m.write32(0xe000e014, 9).unwrap();
        m.write32(0xe000e018, 0).unwrap();
        m.write32(0xe000e010, source | 1).unwrap();
        m.run_until(1000, 3000).unwrap();
        assert_eq!(m.read32(0xe000e010).unwrap() & (1 << 16), flag);
        assert_eq!(m.read32(0xe000e010).unwrap() & (1 << 16), 0);
    }
}

#[test]
fn h750_priorities_and_icsr_pending_fields_follow_the_st_register_contract() {
    let mut m = Machine::from_binary(Board::Rev5, &firmware()).unwrap();
    m.write32(0xe000e400, 0x12345678).unwrap();
    assert_eq!(m.read32(0xe000e400).unwrap(), 0x10305070);
    m.pend_interrupt(0).unwrap();
    assert_ne!(m.read32(0xe000ed04).unwrap() & (1 << 22), 0);
    m.write32(0xe000e100, 1).unwrap();
    assert_eq!(m.read32(0xe000ed04).unwrap() & 0x1ff800, 0x10800);
    for bank in [5, 15] {
        m.write32(0xe000e100 + bank * 4, u32::MAX).unwrap();
        m.write32(0xe000e200 + bank * 4, u32::MAX).unwrap();
        assert_eq!(m.read32(0xe000e100 + bank * 4).unwrap(), 0);
        assert_eq!(m.read32(0xe000e200 + bank * 4).unwrap(), 0);
    }
}

#[test]
fn guest_sysreset_reboots_the_cpu_and_keeps_ram_and_external_flash() {
    let mut code = vec![0; 0xac];
    code[..4].copy_from_slice(&0x20010000u32.to_le_bytes());
    code[4..8].copy_from_slice(&0x08000081u32.to_le_bytes());
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
    let mut m = Machine::from_binary(Board::Rev5, &code).unwrap();
    let mut chip = virtual_daisy::nor::NorFlash::default();
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(2, 0, &[0xa5], 0, false).unwrap();
    chip.advance_time(1_000_000).unwrap();
    let flash = chip.nonvolatile_image();
    m.restore_nonvolatile_image(&flash).unwrap();
    m.write32(0xe000ed0c, 0x12340004).unwrap(); // Wrong key has no effect.
    m.write32(0xe000ed0c, 0x05fa0500).unwrap(); // Priority grouping, no reset.
    assert_eq!(m.read32(0xe000ed0c).unwrap(), 0xfa050500);
    m.run_until(10_000, 5000).unwrap();
    assert_eq!(m.resets(), 1);
    assert_eq!(m.read32(0x20000000).unwrap(), 2);
    assert_eq!(m.read32(0xe000ed0c).unwrap(), 0xfa050000);
    assert_eq!(m.nonvolatile_image(), flash);
    assert!(m.restore_nonvolatile_image(&flash).is_err());
}

fn codec_register(m: &mut Machine, register: u32, value: u32) {
    m.write32(0x580244e8, 1 << 22).unwrap();
    m.write32(0x40005800, 1).unwrap();
    m.write32(0x40005804, (0x1a << 1) | (2 << 16) | (1 << 25) | (1 << 13))
        .unwrap();
    m.write32(0x40005828, (register << 1) | ((value >> 8) & 1))
        .unwrap();
    m.write32(0x40005828, value & 255).unwrap();
}

#[test]
fn physical_codec_profiles_preserve_stereo_and_clock_frames() {
    for board in Board::ALL {
        let mut m = Machine::from_binary(board, &firmware()).unwrap();
        if board == Board::Rev5 {
            for (reg, value) in [
                (0, 23),
                (1, 23),
                (4, 0x12),
                (5, 0),
                (6, 0x42),
                (7, 9 | (1 << 5)),
                (8, 0),
                (9, 1),
            ] {
                codec_register(&mut m, reg, value);
            }
        } else if board == Board::Rev4 {
            m.write32(0x580244e0, 1 << 1).unwrap();
            m.write32(0x58020400, 1 << 22).unwrap();
            m.write32(0x58020418, 1 << 11).unwrap();
        }
        m.write32(0x580244f0, 1 << 22).unwrap();
        m.write32(0x58024450, 4).unwrap();
        let rx = board.rx_channel();
        for channel in 0..2 {
            let base = 0x40015800 + channel * 32;
            m.write32(base + 0xc, 63 | (31 << 8) | (1 << 16) | (1 << 18))
                .unwrap();
            m.write32(base + 0x10, 0x30180).unwrap();
            m.write32(
                base + 4,
                (4 << 20)
                    | (6 << 5)
                    | (channel << 1)
                    | u32::from(channel == rx)
                    | if channel == 1 { 1 << 10 } else { 0 },
            )
            .unwrap();
        }
        let tx = 0x40015820 + (1 - rx) * 32;
        m.write32(tx, 0x00100000).unwrap();
        m.write32(tx, 0x00e00000).unwrap();
        m.input(&[[0.5, -0.75]]).unwrap();
        for channel in [1, 0] {
            let cr = 0x40015804 + channel * 32;
            let value = m.read32(cr).unwrap();
            m.write32(cr, value | (1 << 16)).unwrap();
        }
        m.run_until(16_000, 5000).unwrap();
        assert_eq!(m.sample_rate().unwrap().hz(), 62_500.0);
        assert_eq!(
            m.take_audio(),
            if board == Board::Rev5 {
                vec![[-0.25, 0.125]]
            } else {
                vec![[0.125, -0.25]]
            }
        );
        let dr = 0x40015820 + rx * 32;
        assert_eq!(m.read32(dr).unwrap(), 0x00400000);
        assert_eq!(m.read32(dr).unwrap(), 0xffa00000);
    }
}

#[test]
fn sai_reset_keeps_undrained_audio_and_fifo_error_history() {
    let mut m = Machine::from_binary(Board::Rev7, &firmware()).unwrap();
    m.write32(0x580244f0, 1 << 22).unwrap();
    m.write32(0x58024450, 4).unwrap();
    m.write32(0x4001580c, 63).unwrap();
    m.write32(0x40015820, 0x00100000).unwrap();
    m.write32(0x40015820, 0x00e00000).unwrap();
    m.write32(0x40015804, (4 << 20) | (6 << 5) | (1 << 16))
        .unwrap();
    m.run_until(32_000, 5000).unwrap();
    assert_eq!(m.read32(0x40015818).unwrap() & 1, 1);
    m.write32(0x4001581c, 1).unwrap();
    assert_eq!(m.read32(0x40015818).unwrap() & 1, 0);
    m.write32(0x58024498, 1 << 22).unwrap(); // RCC APB2 reset, SAI1 only.
    assert_eq!(m.take_audio(), [[0.125, -0.25], [0., 0.]]);
    assert!(m.take_audio().is_empty());
    let stats = m.audio_capture_stats();
    assert_eq!(stats.first_rate.unwrap().hz(), 62_500.);
    assert_eq!((stats.frames, stats.underruns, stats.overruns), (2, 1, 0));
    assert!(!stats.mixed_sample_rates);
}

#[test]
fn capture_history_remembers_a_transient_change_of_frame_rate() {
    let mut m = Machine::from_binary(Board::Rev7, &firmware()).unwrap();
    m.write32(0x580244f0, 1 << 22).unwrap();
    m.write32(0x58024450, 4).unwrap();
    m.write32(0x4001580c, 63).unwrap();
    for (divider, end) in [(4, 16_000), (5, 36_000), (4, 52_000)] {
        m.write32(0x40015804, 6 << 5).unwrap();
        m.write32(0x40015820, 0).unwrap();
        m.write32(0x40015820, 0).unwrap();
        m.write32(0x40015804, (divider << 20) | (6 << 5) | (1 << 16))
            .unwrap();
        m.run_until(end, 5000).unwrap();
    }
    let stats = m.audio_capture_stats();
    assert_eq!(stats.first_rate, m.sample_rate());
    assert_eq!(stats.frames, 3);
    assert!(stats.mixed_sample_rates);
}

#[test]
fn a_stalled_thumb32_flash_fetch_does_not_restart_its_first_half_forever() {
    let mut bytes = firmware();
    // BL to the existing marker-writing sequence; the return is never taken.
    bytes[0x80..0x84].copy_from_slice(&[0x00, 0xf0, 0x0e, 0xf8]);
    bytes.resize(0xb8, 0);
    bytes[0xa0..0xa8].copy_from_slice(&[3, 0x48, 4, 0x49, 1, 0x60, 0xfe, 0xe7]);
    bytes[0xb0..0xb4].copy_from_slice(&0x20000000u32.to_le_bytes());
    bytes[0xb4..0xb8].copy_from_slice(&0x12345678u32.to_le_bytes());
    let mut m = Machine::from_binary(Board::Rev5, &bytes).unwrap();
    m.run_until(10_000, 2000).unwrap();
    assert_eq!(m.read32(0x20000000).unwrap(), 0x12345678);
}
