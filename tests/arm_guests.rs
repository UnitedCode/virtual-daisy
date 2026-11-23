#![cfg(feature = "rust-host")]
//! The same unmodified ARM guests run by the Renode reference harness.
use std::{fs, path::PathBuf, process::Command};
use virtual_daisy::{board::Board, machine::Machine};

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

fn guest(name: &str) -> (PathBuf, Vec<u8>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let directory = std::env::temp_dir().join(format!("daisy-{name}-guest-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
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
        .arg(root.join(format!("guests/{name}.rs")))
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
        let words: Vec<_> = (0..96)
            .map(|i| machine.read32(0x20018000 + i * 4).unwrap())
            .collect();
        eprintln!(
            "CPU results: {words:08x?}; registers={:08x?}",
            machine.registers()
        );
        assert_eq!(words[0], 0xab, "guest completion");
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
                if eager { 352 } else { 136 }
            );
        }
        if let Some(reference) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
            let data = fs::read(PathBuf::from(reference).join(format!("{label}.cpu.bin"))).unwrap();
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
