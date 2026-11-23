#![cfg(feature = "rust-host")]
//! The same unmodified ARM guests run by the Renode reference harness.
use std::{fs, path::PathBuf, process::Command};
use virtual_daisy::{board::Board, machine::Machine};
#[path = "../cpu/fixtures/divide_probe_cases.rs"]
mod divide_cases;
#[path = "../src/events_probe.rs"]
mod events_probe;
#[path = "../cpu/fixtures/fixed_probe.rs"]
mod fixed_probe;
#[path = "../cpu/fixtures/half_probe.rs"]
mod half_probe;
#[path = "../src/instruction_clock_probe.rs"]
mod instruction_clock_probe;
#[path = "../cpu/fixtures/long_multiply_probe.rs"]
mod long_multiply_probe;
#[path = "../cpu/fixtures/psr_probe.rs"]
mod psr_probe;
#[path = "../cpu/fixtures/saturation_probe.rs"]
mod saturation_probe;
#[path = "../src/unprivileged_probe.rs"]
mod unprivileged_probe;

#[test]
fn old_reference_user_stir_bypass_fails_the_privilege_gate() {
    let bytes = include_bytes!("fixtures/unprivileged_legacy_reference.bin");
    assert!(unprivileged_probe::check(bytes)
        .unwrap_err()
        .starts_with("ARM unprivileged case 72 opcode 0xf8410e00:"));
    let at = (16 + 72 * 8) * 4;
    assert_eq!(
        u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()),
        32
    );
    assert_eq!(
        u32::from_le_bytes(bytes[at + 20..at + 24].try_into().unwrap()),
        0
    );
}

#[test]
fn old_native_transfers_and_disabled_stir_fail_the_privilege_gate() {
    let bytes = include_bytes!("fixtures/unprivileged_legacy_native.bin");
    assert!(unprivileged_probe::check(bytes)
        .unwrap_err()
        .starts_with("ARM unprivileged guest did not complete:"));
    assert_eq!(u32::from_le_bytes(bytes[60..64].try_into().unwrap()), 21);
    // The first byte load was undefined, rather than a denied unprivileged read.
    assert_eq!(
        u32::from_le_bytes(bytes[116..120].try_into().unwrap()),
        1 << 16
    );
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn unprivileged_guest_checks_mpu_permissions_alignment_and_user_stir() {
    let (directory, image) = guest("unprivileged");
    let mut machine = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    machine.run_until(20_000_000, 3_000_000).unwrap();
    let data: Vec<_> = (0..unprivileged_probe::WORDS as u32)
        .flat_map(|i| machine.read32(0x20019800 + i * 4).unwrap().to_le_bytes())
        .collect();
    unprivileged_probe::check(&data).unwrap();
    if let Some(path) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
        let reference =
            fs::read(PathBuf::from(path).join("unprivileged.unprivileged.bin")).unwrap();
        unprivileged_probe::check(&reference).unwrap();
        assert_eq!(data, reference);
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn old_reference_sevonpend_bank_fails_the_event_gate() {
    let bytes = include_bytes!("fixtures/events_legacy_reference.bin");
    assert!(events_probe::check(bytes)
        .unwrap_err()
        .starts_with("ARM event guest did not complete:"));
    let stages: Vec<_> = bytes[64..100]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    assert_eq!(stages, [1, 1, 1, 1, 1, 3, 5, 7, 7]);
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn event_guest_checks_pending_edges_masks_wfe_wfi_and_exception_return() {
    let (directory, image) = guest("events");
    let mut machine = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    let mut deadline = 0;
    for (index, expected) in events_probe::STAGES.into_iter().enumerate() {
        match index {
            1 | 2 | 8 => machine.pend_interrupt(5).unwrap(),
            3 | 4 => {
                machine.write32(0xe000e280, 1 << 5).unwrap();
                machine.pend_interrupt(5).unwrap();
            }
            5..=7 => machine.signal_event(),
            _ => {}
        }
        deadline += 1_000_000;
        machine.run_until(deadline, 200_000).unwrap();
        let stage = machine.read32(0x20019804).unwrap();
        assert_eq!(stage, expected, "checkpoint {index}");
        machine
            .write32(0x20019840 + index as u32 * 4, stage)
            .unwrap();
    }
    let data: Vec<_> = (0..32)
        .flat_map(|i| machine.read32(0x20019800 + i * 4).unwrap().to_le_bytes())
        .collect();
    events_probe::check(&data).unwrap();
    if let Some(path) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
        let path = PathBuf::from(path);
        let reference = fs::read(path.join("events.events.bin")).unwrap();
        events_probe::check(&reference).unwrap();
        assert_eq!(data, reference);
        assert!(events_probe::check(
            &fs::read(path.join("events-noop-control.events.bin")).unwrap()
        )
        .is_err());
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn old_reference_unsigned_exchange_flags_fail_the_register_gate() {
    let bytes = include_bytes!("../cpu/fixtures/parallel_legacy_reference.bin");
    assert_eq!(
        saturation_probe::check(bytes).unwrap_err(),
        "ARM saturation failed at case 31 opcode 0xfaa2f040 register 14 actual 0xa8000000"
    );
}

#[test]
fn old_reference_uncached_blocks_fail_the_instruction_width_gate() {
    assert_eq!(
        instruction_clock_probe::check(include_bytes!(
            "fixtures/uncached_fetch_legacy_reference.bin"
        ))
        .unwrap_err(),
        "ARM instruction fetch: 128 uncached NOPs advanced Thumb-16=184, Thumb-32=224 cycles"
    );
}
#[path = "../src/systick_probe.rs"]
mod systick_probe;

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn long_multiply_guest_checks_all_output_pairs_and_source_aliases() {
    let (directory, image) = guest("long_multiply");
    let mut machine = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    machine.run_until(1_200_000_000, 144_000_000).unwrap();
    let data: Vec<_> = (0..32)
        .flat_map(|i| machine.read32(0x20018800 + i * 4).unwrap().to_le_bytes())
        .collect();
    long_multiply_probe::check(&data).unwrap();
    if let Some(path) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
        long_multiply_probe::check(
            &fs::read(PathBuf::from(path).join("long-multiply.long-multiply.bin")).unwrap(),
        )
        .unwrap();
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn psr_guest_checks_masked_alias_writes_and_unprivileged_exception_preservation() {
    let (directory, image) = guest("psr");
    let mut machine = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    machine.run_until(10_000_000, 2_000_000).unwrap();
    let data: Vec<_> = (0..32)
        .flat_map(|i| machine.read32(0x20018000 + i * 4).unwrap().to_le_bytes())
        .collect();
    psr_probe::check(&data).unwrap();
    if let Some(path) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
        psr_probe::check(&fs::read(PathBuf::from(path).join("psr.psr.bin")).unwrap()).unwrap();
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn saturation_guest_executes_all_register_pairs_including_lr() {
    let (directory, image) = guest("saturation");
    let mut machine = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    // The packed-DSP additions more than triple the register matrix.
    machine.run_until(1_600_000_000, 192_000_000).unwrap();
    let data: Vec<_> = (0..32)
        .flat_map(|i| machine.read32(0x20018400 + i * 4).unwrap().to_le_bytes())
        .collect();
    saturation_probe::check(&data).unwrap();
    if let Some(path) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
        let path = PathBuf::from(path);
        saturation_probe::check(&fs::read(path.join("saturation.saturation.bin")).unwrap())
            .unwrap();
        let error = saturation_probe::check(
            &fs::read(path.join("saturation-psr-control.saturation.bin")).unwrap(),
        )
        .unwrap_err();
        assert!(error.contains("register 14 actual 0xa8000000"));
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn fixed_guest_executes_signed_unsigned_scaling_and_register_preservation() {
    let (directory, image) = guest("fixed");
    let mut machine = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    machine.run_until(100_000_000, 12_000_000).unwrap();
    let data: Vec<_> = (0..32)
        .flat_map(|i| machine.read32(0x20018800 + i * 4).unwrap().to_le_bytes())
        .collect();
    fixed_probe::check(&data).unwrap();
    if let Some(path) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
        fixed_probe::check(&fs::read(PathBuf::from(path).join("fixed.fixed.bin")).unwrap())
            .unwrap();
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn half_guest_executes_storage_formats_and_direct_double_rounding() {
    let (directory, image) = guest("half");
    let mut machine = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    machine.run_until(500_000_000, 40_000_000).unwrap();
    let data: Vec<_> = (0..32)
        .flat_map(|i| machine.read32(0x20018c00 + i * 4).unwrap().to_le_bytes())
        .collect();
    half_probe::check(&data).unwrap();
    if let Some(path) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
        let path = PathBuf::from(path);
        half_probe::check(&fs::read(path.join("half.half.bin")).unwrap()).unwrap();
        half_probe::check_legacy_double(
            &fs::read(path.join("half-double-control.half.bin")).unwrap(),
            &fs::read(path.join("half-double-control.half-site.bin")).unwrap(),
        )
        .unwrap();
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn instruction_clock_tracks_progress_inside_a_literal_itcm_sequence() {
    let (directory, image) = guest("instruction_clock");
    let mut machine = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    machine.run_until(2_000_000, 1_000_000).unwrap();
    let data: Vec<_> = (0..32)
        .flat_map(|i| machine.read32(0x20019000 + i * 4).unwrap().to_le_bytes())
        .collect();
    instruction_clock_probe::check(&data).unwrap();
    if let Some(path) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
        instruction_clock_probe::check(
            &fs::read(PathBuf::from(path).join("instruction-clock.instruction-clock.bin")).unwrap(),
        )
        .unwrap();
    }
    fs::remove_dir_all(directory).unwrap();
}

fn counter(machine: &Machine, key: &str) -> u64 {
    let report = machine.memory_report();
    let prefix = format!("{key}=");
    report
        .lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .unwrap()
        .parse()
        .unwrap()
}
#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn systick_guest_checks_both_sources_against_the_independent_dwt_clock() {
    let (directory, image) = guest("systick");
    let mut m = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    m.run_until(2_000_000, 1_000_000).unwrap();
    let data: Vec<_> = (0..32)
        .flat_map(|i| m.read32(0x20019f00 + i * 4).unwrap().to_le_bytes())
        .collect();
    systick_probe::check(&data).unwrap();
    if let Some(path) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
        systick_probe::check(&fs::read(PathBuf::from(path).join("systick.systick.bin")).unwrap())
            .unwrap();
    }
    fs::remove_dir_all(directory).unwrap();
}

fn guest(name: &str) -> (PathBuf, Vec<u8>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let directory = std::env::temp_dir().join(format!("daisy-{name}-guest-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let source = directory.join(format!("{name}.rs"));
    let content = fs::read_to_string(root.join(format!("guests/{name}.rs"))).unwrap();
    fs::write(
        &source,
        content
            .replace(
                "// UNPRIVILEGED_CASES",
                include_str!("../cpu/fixtures/unprivileged_probe_cases.rs"),
            )
            .replace(
                "// LONG_MULTIPLY_CASES",
                include_str!("../cpu/fixtures/long_multiply_probe_cases.rs"),
            )
            .replace(
                "// EXTEND_PACK_CASES",
                include_str!("../cpu/fixtures/extend_pack_probe_cases.rs"),
            )
            .replace(
                "// DSP_MULTIPLY_CASES",
                include_str!("../cpu/fixtures/dsp_multiply_probe_cases.rs"),
            )
            .replace(
                "// PARALLEL_CASES",
                include_str!("../cpu/fixtures/parallel_probe_cases.rs"),
            )
            .replace(
                "// WORD_CASES",
                include_str!("../cpu/fixtures/qadd_probe_cases.rs"),
            )
            .replace(
                "// PSR_CASES",
                include_str!("../cpu/fixtures/psr_probe_cases.rs"),
            )
            .replace(
                "// SATURATION_CASES",
                include_str!("../cpu/fixtures/saturation_probe_cases.rs"),
            )
            .replace(
                "// FIXED_CASES",
                include_str!("../cpu/fixtures/fixed_probe_cases.rs"),
            ),
    )
    .unwrap();
    let elf = directory.join(format!("{name}.elf"));
    assert!(Command::new("rustc")
        .args([
            "--edition=2021",
            "--target",
            "thumbv7em-none-eabihf",
            "-C",
            "target-cpu=cortex-m7",
            "-C",
            "opt-level=s",
            "-C",
            "panic=abort",
            "-C"
        ])
        .arg(format!(
            "link-arg=-T{}",
            root.join("guests/cpu.ld").display()
        ))
        .arg("--crate-name")
        .arg(format!("daisy_{name}_probe"))
        .arg(source)
        .arg("-o")
        .arg(&elf)
        .status()
        .unwrap()
        .success());
    let bytes = fs::read(elf).unwrap();
    (directory, bytes)
}
fn compare(machine: &mut Machine, address: u32, count: u32, filename: &str) {
    let words: Vec<_> = (0..count)
        .map(|i| machine.read32(address + i * 4).unwrap())
        .collect();
    eprintln!("{filename}: {words:08x?}");
    if filename.ends_with(".fetch.bin") {
        assert_eq!(words[1], 3);
        for (phase, pc) in [(0, 0x90000040), (1, 0x9000007e), (2, 0x90000040)] {
            let at = 4 + phase * 4;
            assert_eq!(&words[at..at + 4], &[5, 0x100, pc, 0]);
        }
        assert_eq!(&words[16..20], &[0x5a, 0x5a, 0x5a, 0x2000f8c1]);
    } else if filename.ends_with(".sai.bin") {
        assert_eq!(&words[1..9], &[1, 2, 103, 0, 0, 0x40, 0x40, 1]);
        assert_eq!(words[31], 0);
    } else if filename.ends_with(".fault.bin") {
        assert_eq!(words[1], 8);
        for (phase, status, addr) in [
            (1, 0x8200, 0x90000000),
            (2, 0x8200, 0x90000000),
            (3, 0x8200, 0x90800000),
            (4, 0x100, 0x90000000),
            (5, 0x100, 0x90800000),
            (6, 0x8200, 0x90000000),
            (7, 0x8200, 0x52002004),
            (8, 0x8200, 0x52002004),
        ] {
            let at = 4 + (phase - 1) * 4;
            assert_eq!(words[at], if phase == 6 { 3 } else { 5 });
            assert_eq!(words[at + 1], status);
            assert_eq!(words[at + if matches!(phase, 4 | 5) { 3 } else { 2 }], addr);
        }
        assert_eq!(
            &words[48..52],
            if filename.starts_with("bus-dma-valid") {
                &[0x30, 0x5054, 0, 1]
            } else {
                &[8, 0x5054, 1, 1]
            }
        );
        assert_eq!(words[63], 0);
    } else if filename.ends_with(".vector.bin") {
        assert_eq!((words[1], words[5], words[6], words[7]), (1, 3, 2, 0));
        assert_eq!(words[4], words[3]);
        assert_eq!(words[9], words[3] - 32);
        assert_eq!((words[8], words[13], words[14]), (0xfffffff9, 0, 1));
        assert_eq!(words[31], 0);
    } else if filename.ends_with(".sdram.bin") {
        for (at, value) in [
            (1, 1),
            (2, 1),
            (3, 64),
            (4, 0),
            (5, 82),
            (6, 0x12340000),
            (7, 0x12340001),
            (8, 0x12340002),
            (10, 0x2d0),
            (11, 0x0fffffff),
            (12, 0x8200),
            (13, 0xc0400000),
            (14, 5),
            (15, 1),
            (31, 0),
        ] {
            assert_eq!(words[at], value, "SDRAM word {at}");
        }
    } else if filename.ends_with(".transition.bin") {
        assert_eq!((words[1], words[2], words[5], words[31]), (2, 3, 0, 0));
        assert_eq!(words[3], words[4]);
        assert_eq!(words[27], words[28]);
        assert_eq!(words[29], words[30]);
        let order = if filename.contains("equal") || filename.contains("nested") {
            [16, 17]
        } else {
            [17, 16]
        };
        assert_eq!([words[8], words[16]], order);
        if !filename.contains("nested") {
            assert_eq!(words[14], words[22]);
        }
        if filename.contains("-fp") {
            assert_eq!(&words[24..26], &[0x3fc00000, 0x00400000]);
            assert_eq!(words[26] & 9, 8);
        }
    }
    if let Some(reference) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
        let data = fs::read(PathBuf::from(reference).join(filename)).unwrap();
        assert_eq!(data.len(), words.len() * 4, "{filename} reference length");
        for (i, b) in data.as_chunks::<4>().0.iter().enumerate() {
            // Renode omits ICSR pending fields in these fixtures. Verify the
            // common active/frame fields here; direct tests cover the ST fields.
            let mask = if filename == "transition-nested.transition.bin" && i == 22 {
                assert!(
                    words[i] & 1 == 0 && (0xa000..0xf000).contains(&words[i]),
                    "nested IRQ interrupted valid probe code"
                );
                0 // A timed arrival can stop at a different instruction in the polling loop.
            } else if filename.ends_with(".transition.bin") && i == 26 {
                if filename.contains("-fp") {
                    assert_eq!(words[i] & 8, 8, "FP frame was allocated in Thread mode");
                }
                !8
            } else if filename.ends_with(".transition.bin") && matches!(i, 12 | 20)
                || filename.ends_with(".vector.bin") && i == 12
            {
                0x9ff
            } else {
                u32::MAX
            };
            assert_eq!(
                words[i] & mask,
                u32::from_le_bytes(*b) & mask,
                "{filename} reference word {i}"
            );
        }
    }
    assert_eq!(words[0], 0xab, "{filename} completion");
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn qspi_fetch_guest_preserves_valid_prefixes_and_precise_faults() {
    let (directory, image) = guest("fetch");
    let mut m = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    m.run_until(2_000_000, 1_000_000).unwrap();
    compare(&mut m, 0x20019400, 32, "fetch.fetch.bin");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn peripheral_bus_fault_guest_and_dma_error_match_reference() {
    let (directory, image) = guest("bus");
    for label in [
        "bus",
        "bus-dma-valid",
        "bus-dma-disable",
        "bus-dma-abort",
        "bus-dma-range",
    ] {
        let mut m = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
        m.run_until(2_000_000, 1_000_000).unwrap();
        if label != "bus" {
            m.write32(0x52005000, 1 | (3 << 24)).unwrap();
            m.write32(0x52005014, 0x0d002503).unwrap();
        }
        if label == "bus-dma-range" {
            let cr = m.read32(0x40020010).unwrap();
            m.write32(0x40020010, cr & !1).unwrap();
            m.write32(0x4002001c, 0x90800000).unwrap();
            m.write32(0x40020010, cr).unwrap();
        }
        m.request_dma(0).unwrap();
        if matches!(label, "bus-dma-disable" | "bus-dma-abort") {
            m.run_until(2_002_000, 1_000_000).unwrap();
            m.write32(
                0x52005000,
                if label == "bus-dma-disable" {
                    0
                } else {
                    3 | (3 << 24)
                },
            )
            .unwrap();
        }
        m.run_until(2_020_000, 1_000_000).unwrap();
        compare(&mut m, 0x20019000, 64, &format!("{label}.fault.bin"));
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn sai_interrupt_guest_matches_reference() {
    let (directory, image) = guest("sai");
    let mut m = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    m.write32(0x20019d80, 0).unwrap();
    m.run_until(100_000, 1_000_000).unwrap();
    m.write32(0x40015814, 2).unwrap();
    m.write32(0xe000e108, 1 << 23).unwrap();
    m.run_until(200_000, 1_000_000).unwrap();
    compare(&mut m, 0x20019d00, 32, "sai.sai.bin");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn sdram_geometry_and_refresh_irq_match_reference() {
    let (directory, image) = guest("sdram");
    let mut m = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
    m.run_until(100_000, 1_000_000).unwrap();
    m.inject_contention(5, 100_000, false).unwrap();
    m.run_until(300_000, 1_000_000).unwrap();
    compare(&mut m, 0x20019c00, 32, "sdram.sdram.bin");
    for key in [
        "sdram_activations",
        "sdram_precharges",
        "sdram_refreshes",
        "sdram_fifo_hits",
    ] {
        let report = m.memory_report();
        let prefix = format!("{key}=");
        let value = report
            .lines()
            .find_map(|l| l.strip_prefix(&prefix))
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!(value > 0, "{key} was exercised");
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn vector_fault_reuses_the_original_exception_frame() {
    let (directory, image) = guest("vector");
    for (label, mode) in [("vector", 0), ("vector-cached", 1), ("vector-enabled", 4)] {
        let mut m = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
        m.write32(0x20019900, mode).unwrap();
        m.run_until(2_000_000, 1_000_000).unwrap();
        m.pend_interrupt(0).unwrap();
        m.run_until(2_100_000, 1_000_000)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        compare(&mut m, 0x20019800, 32, &format!("{label}.vector.bin"));
        assert_eq!(
            counter(&m, "exception_stack_words"),
            if mode == 1 { 16 } else { 8 }
        );
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn late_arrival_and_tail_chaining_preserve_the_thread_frame() {
    let (directory, image) = guest("transition");
    for (label, mode, tail) in [
        ("transition-late", 0, false),
        ("transition-equal", 1, false),
        ("transition-tail", 0, true),
        ("transition-fp", 8, true),
        ("transition-psp", 16, true),
        ("transition-fp-psp", 24, true),
        ("transition-nested", 2, false),
        ("transition-return", 4, false),
    ] {
        let mut m = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
        m.write32(0x20019700, mode).unwrap();
        m.run_until(1_000_000, 1_000_000).unwrap();
        m.inject_contention(8, 100_000, true).unwrap();
        m.pend_interrupt(usize::from(label == "transition-return"))
            .unwrap();
        if tail {
            m.pend_interrupt(1).unwrap();
        }
        m.run_until(1_002_000, 1_000_000).unwrap();
        assert_eq!(
            m.read32(0x20019604).unwrap(),
            0,
            "no handler before stack service"
        );
        let end = if label == "transition-nested" || label == "transition-return" {
            m.run_until(1_152_000, 1_000_000).unwrap();
            if label == "transition-return" {
                m.inject_contention(8, 100_000, false).unwrap();
                m.write32(0x20019704, 1).unwrap();
                m.run_until(1_154_000, 1_000_000).unwrap();
                m.pend_interrupt(0).unwrap();
                1_654_000
            } else {
                m.pend_interrupt(1).unwrap();
                1_652_000
            }
        } else {
            if !tail {
                m.pend_interrupt(1).unwrap();
            }
            1_502_000
        };
        m.run_until(end, 1_000_000)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        compare(&mut m, 0x20019600, 32, &format!("{label}.transition.bin"));
        assert_eq!(
            counter(&m, "exception_stack_words"),
            if label == "transition-nested" { 16 } else { 8 },
            "{label} stacks exactly the required frames"
        );
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires rustup target add thumbv7em-none-eabihf"]
fn architectural_guest_matches_reference_results() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let directory = std::env::temp_dir().join(format!("daisy-arm-guest-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let elf = directory.join("cpu.elf");
    let status = Command::new("rustc")
        .args([
            "--edition=2021",
            "--target",
            "thumbv7em-none-eabihf",
            "-C",
            "target-cpu=cortex-m7",
            "-C",
            "opt-level=s",
            "-C",
            "panic=abort",
            "-C",
        ])
        .arg(format!(
            "link-arg=-T{}",
            root.join("guests/cpu.ld").display()
        ))
        .arg("--crate-name")
        .arg("daisy_cpu_probe")
        .arg(root.join("guests/cpu.rs"))
        .arg("-o")
        .arg(&elf)
        .status()
        .unwrap();
    assert!(status.success());
    let image = fs::read(&elf).unwrap();
    for (label, shared, eager) in [
        ("cpu-probes", false, false),
        ("cpu-ram", true, false),
        ("cpu-eager", false, true),
        ("cpu-eager-ram", true, true),
    ] {
        let mut image = image.clone();
        if shared {
            let word = |o: usize| u32::from_le_bytes(image[o..o + 4].try_into().unwrap()) as usize;
            let phoff = word(28);
            let size = word(40) >> 16;
            let count = word(44) & 65535;
            let at = (0..count)
                .map(|i| phoff + i * size)
                .find(|&at| {
                    word(at) == 1
                        && word(at + 12) <= 0x6000
                        && word(at + 12) + word(at + 16) >= 0x6004
                })
                .unwrap();
            let offset = word(at + 4) + 0x6000 - word(at + 12);
            image[offset..offset + 4].copy_from_slice(&0x3001f000u32.to_le_bytes());
        }
        let mut machine = Machine::from_elf_at(Board::Rev5, &image, 0x6000).unwrap();
        machine
            .write32(0x20018200, if eager { 4 } else { 0 })
            .unwrap();
        machine
            .run_until(2_000_000, 1_000_000)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        let words: Vec<_> = (0..116)
            .map(|i| machine.read32(0x20018000 + i * 4).unwrap())
            .collect();
        eprintln!(
            "CPU results: {words:08x?}; registers={:08x?}",
            machine.registers()
        );
        assert_eq!(words[0], 0xab, "guest completion");
        assert_eq!(&words[104..108], &[0x40200, 0x40200, 0x7031b, 0x7031b]);
        assert_eq!(words[108], 0);
        assert_eq!(words[109] & 7, 0);
        assert_ne!(words[110] & (1 << 9), 0);
        assert_eq!(words[111], 0);
        assert_eq!(&words[112..116], &[0xa80a0000, 0xfffffff0, 4, 0x48494e54]);
        assert_eq!(
            &words[96..100],
            &[0x7f800001, 0x80000000, 0xffc01234, 0x80000000]
        );
        for (i, case) in divide_cases::CPU_DIVIDE_CASES.into_iter().enumerate() {
            assert_eq!(words[100 + i], case[4]);
        }
        assert_eq!(words[1], 16, "fault count");
        for (phase, exception, status) in [
            (1, 6, 0x02000000),
            (2, 6, 0x01000000),
            (3, 4, 0x82),
            (4, 4, 1),
            (5, 6, 0x80000),
            (7, 6, 0x01000000),
            (8, 6, 0x01000000),
            (9, 6, 0x01000000),
            (10, 6, 0x01000000),
            (11, 6, 0x01000000),
            (12, 6, 0x01000000),
            (13, 6, 0x01000000),
            (14, 6, 0x01000000),
            (15, 6, 0x01000000),
            (16, 6, 0x01000000),
            (17, 6, 0x01000000),
        ] {
            let at = if phase <= 5 {
                4 + (phase - 1) * 4
            } else if phase <= 10 {
                24 + (phase - 7) * 4
            } else {
                64 + (phase - 11) * 4
            };
            assert_eq!(
                (words[at], words[at + 1]),
                (exception, status),
                "{label} phase {phase}"
            );
        }
        for (at, value) in [
            (2, 0x1000),
            (3, 0x10110221),
            (14, 0x24040000),
            (19, 0x24040000),
            (42, 0x3fc00000),
            (43, 0x00400000),
            (44, 0xffffffe9),
            (46, 0),
            (49, 0x3fc00000),
            (54, 0),
            (55, 0x400e0000),
            (58, 0x12000011),
            (59, 0x40),
            (60, 0x55443322),
            (61, 0x44332211),
            (62, 0),
            (63, 0),
            (92, 0x44332211),
            (93, 0x88776655),
            (94, 0xccbbaa99),
            (95, 0x88888888),
        ] {
            assert_eq!(words[at], value, "{label} word {at}");
        }
        assert_eq!(words[40], words[41]);
        assert_eq!(words[48], words[40] - 104);
        assert_eq!(words[45], u32::from(!eager));
        for (i, (at, expected)) in [7, 11, 15, 23, 27, 31, 35, 39, 67, 71, 75, 79, 83, 87, 91]
            .into_iter()
            .zip([
                0xf0f2fbb1, 0x6808, 0x6808, 0x0a00eeb7, 0xc905, 0xc105, 0x0200e9d1, 0x0200e9c1,
                0x0a00edd1, 0x0a00ed81, 0x1a02ecb1, 0x0a02eca1, 0x6808, 0x0f00e851, 0x0200e841,
            ])
            .enumerate()
        {
            let mask = if matches!(i, 1 | 2 | 4 | 5 | 12) {
                0xffff
            } else {
                u32::MAX
            };
            assert_eq!(
                machine.read32(words[at]).unwrap() & mask,
                expected,
                "{label} precise fault instruction {i}"
            );
        }
        if shared {
            assert_eq!(
                counter(&machine, "exception_stack_words"),
                if eager { 360 } else { 144 }
            );
        }
        if let Some(reference) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
            let data = fs::read(PathBuf::from(reference).join(format!("{label}.cpu.bin"))).unwrap();
            assert_eq!(data.len(), words.len() * 4, "{label} reference length");
            for (i, b) in data.as_chunks::<4>().0.iter().enumerate() {
                assert_eq!(
                    words[i],
                    u32::from_le_bytes(*b),
                    "{label} reference word {i}"
                );
            }
        }
    }
    fs::remove_dir_all(directory).unwrap();
}
