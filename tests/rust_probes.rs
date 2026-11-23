#![cfg(feature = "rust-host")]
//! Execute the original reference runner's Thumb fixtures on the Rust host.
use virtual_daisy::{board::Board, machine::Machine};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const RATE: u32 = 48_014;
#[allow(dead_code)]
#[path = "../src/grade.rs"]
mod grade;
#[allow(dead_code)]
#[path = "../src/probe.rs"]
mod probe;
#[allow(dead_code)]
#[path = "../src/storage_probe.rs"]
mod storage_probe;

// Put unchanged generated instructions and initial physical memory in ELF load
// segments. Only the vector table belongs to this test harness.
fn machine(entry: u32, sp: u32, segments: &[(u32, &[u8])]) -> Machine {
    let mut vectors = [0; 8];
    vectors[..4].copy_from_slice(&sp.to_le_bytes());
    vectors[4..].copy_from_slice(&(entry | 1).to_le_bytes());
    let all: Vec<_> = std::iter::once((0x08000000, vectors.as_slice()))
        .chain(segments.iter().copied())
        .collect();
    let mut elf = vec![0; 52 + all.len() * 32];
    elf[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    let word = |elf: &mut Vec<u8>, offset: usize, value: u32| {
        elf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    };
    word(&mut elf, 16, 2 | (40 << 16));
    word(&mut elf, 28, 52);
    word(&mut elf, 40, 52 | (32 << 16));
    word(&mut elf, 44, all.len() as u32);
    for (n, (address, data)) in all.into_iter().enumerate() {
        let at = 52 + n * 32;
        let offset = elf.len() as u32;
        for (o, v) in [
            (0, 1),
            (4, offset),
            (8, address),
            (12, address),
            (16, data.len() as u32),
            (20, data.len() as u32),
        ] {
            word(&mut elf, at + o, v);
        }
        elf.extend_from_slice(data);
    }
    Machine::from_elf(Board::Rev5, &elf).unwrap()
}
fn words(m: &mut Machine, address: u32, count: u32) -> Vec<u32> {
    (0..count)
        .map(|i| m.read32(address + i * 4).unwrap())
        .collect()
}
fn counter(m: &Machine, key: &str) -> u64 {
    m.memory_report()
        .lines()
        .find_map(|s| s.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("missing {key}"))
        .parse()
        .unwrap()
}
fn reference(actual: &[u32], filename: &str, timing_words: &[usize]) {
    if let Some(directory) = std::env::var_os("VIRTUAL_DAISY_REFERENCE") {
        let path = std::path::PathBuf::from(directory).join(filename);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(bytes.len(), actual.len() * 4, "{filename} length");
        for (i, (&a, b)) in actual.iter().zip(bytes.as_chunks::<4>().0).enumerate() {
            if !timing_words.contains(&i) {
                assert_eq!(a, u32::from_le_bytes(*b), "{filename} word {i}");
            }
        }
    }
}
fn dwt(m: &mut Machine) {
    for (address, value) in [
        (0xe000edfc, 1 << 24),
        (0xe0001fb0, 0xc5acce55),
        (0xe0001000, 1),
        (0xe0001004, 0),
    ] {
        m.write32(address, value).unwrap();
    }
}

#[test]
fn original_generated_code_keeps_distinct_cpu_dma_and_instruction_views() {
    let mut m = machine(
        0x1000,
        0x2001f000,
        &[
            (0x1000, &probe::image()),
            (0x24040000, &0x47702001u32.to_le_bytes()),
            (0x24042000, &0x47702008u32.to_le_bytes()),
            (0x0801e000, &0x47702009u32.to_le_bytes()),
        ],
    );
    m.write32(0xe000ed14, 3 << 16).unwrap();
    m.run_until(1_000_000, 1_000_000).unwrap();
    assert_eq!(m.read32(0x2001e018).unwrap(), 0x42);
    m.write32(0x24040000, 0x47702003).unwrap();
    m.write32(0x2001e03c, 1).unwrap();
    m.run_until(2_000_000, 1_000_000).unwrap();
    assert_eq!(
        words(&mut m, 0x2001e000, 15),
        [1, 0x47702002, 1, 1, 1, 2, 0x42, 2, 9, 8, 3, 3, 0x43, 15, 1]
    );
    reference(
        &words(&mut m, 0x2001e000, 15),
        "architecture-probes.architecture.bin",
        &[],
    );
}

#[test]
fn original_flash_program_waits_preserve_stm_strd_and_stale_code() {
    let mut m = machine(
        0x2000,
        0x2001f000,
        &[
            (0x2000, &probe::flash_image()),
            (0x0801e000, &0x47702009u32.to_le_bytes()),
        ],
    );
    m.write32(0xe000ed14, 1 << 17).unwrap();
    m.run_until(1_002_000_000, 1_000_000).unwrap();
    assert_eq!(
        words(&mut m, 0x2001d000, 10),
        [9, 5, u32::MAX, 1 << 16, 5, 0x4770202a, 9, 42, 1 << 16, 0x55]
    );
    assert_eq!(counter(&m, "flash_programs"), 1);
    assert_eq!(counter(&m, "flash_erases"), 1);
    assert!(counter(&m, "cpu_bus_wait_ns") >= 1_000_000_000);
    reference(
        &words(&mut m, 0x2001d000, 10),
        "flash-probes.flash.bin",
        &[],
    );
}

#[test]
fn original_dsb_and_busy_flash_vector_preserve_one_sram_frame() {
    let mut m = machine(
        0x3000,
        0x3001f000,
        &[
            (0x3000, &probe::timing_image()),
            (0x0801e02c, &0x3801u32.to_le_bytes()),
        ],
    );
    for (address, value) in [
        (0xe000ed98, 0),
        (0xe000ed9c, 0x3001e000),
        (0xe000eda0, 0x030c0019),
        (0xe000ed94, 5),
        (0xe000ed14, 3 << 16),
        (0xe000ed08, 0x0801e000),
    ] {
        m.write32(address, value).unwrap();
    }
    dwt(&mut m);
    m.inject_contention(7, 100_000, true).unwrap();
    m.run_until(50_000, 1_000_000).unwrap();
    assert_eq!(m.read32(0x2001c000).unwrap(), 0, "DSB waits for writeback");
    m.run_until(500_000, 1_000_000).unwrap();
    let v = words(&mut m, 0x2001c000, 12);
    for (i, expected) in [
        (0, 0xab),
        (3, 99),
        (4, 0x3001efe0),
        (5, 1),
        (6, 0xef),
        (7, 0x3001f000),
        (8, 0x55),
        (9, 0x3102),
        (10, 0x12345678),
        (11, 0x12345678),
    ] {
        assert_eq!(v[i], expected, "timing result {i}: {v:x?}");
    }
    assert!(v[2].wrapping_sub(v[1]) >= 6000);
    // DWT samples depend on instruction costs. Independently require the full
    // service delay above; compare every other word with the reference capture.
    reference(&v, "timing-probes.timing.bin", &[1, 2]);
    assert_eq!(counter(&m, "exception_stack_words"), 8);
}

#[test]
fn original_irq_preempts_and_restarts_flash_and_shared_bus_loads() {
    for shared in [false, true] {
        let code = if shared {
            probe::interrupt_target_image()
        } else {
            probe::interrupt_image()
        };
        let mut m = machine(
            0x5000,
            if shared { 0x3001f000 } else { 0x2001f000 },
            &[(0x5000, &code)],
        );
        m.write32(0x6040, 0x5801).unwrap();
        m.write32(0xe000ed08, 0x6000).unwrap();
        dwt(&mut m);
        if shared {
            m.write32(0x24040000, 42).unwrap();
            m.inject_contention(7, 100_000, false).unwrap();
        }
        m.run_until(20_000, 1_000_000).unwrap();
        m.write32(0xe000e100, 1).unwrap();
        m.pend_interrupt(0).unwrap();
        m.run_until(30_000, 1_000_000).unwrap();
        assert_eq!(m.read32(0x2001b008).unwrap(), 0xab, "shared={shared}");
        m.run_until(500_000, 1_000_000).unwrap();
        let v = words(&mut m, 0x2001b000, 5);
        assert_eq!([v[0], v[1], v[2], v[4]], [42, 0x55, 0xab, 1]);
        assert!(v[3] < 6000);
        reference(
            &v,
            if shared {
                "wait-irq-target.interrupt.bin"
            } else {
                "wait-irq.interrupt.bin"
            },
            &[3],
        );
        assert!(counter(&m, "abandoned_cpu_waits") > 0);
        assert_eq!(
            counter(&m, "exception_stack_words"),
            if shared { 8 } else { 0 }
        );
    }
}

#[test]
fn original_nor_storage_survives_reset_and_reschedules_timeout_irq() {
    let mut m = machine(
        0x9000,
        0x2001f000,
        &[(0x9000, &storage_probe::image(true, false))],
    );
    m.write32(0x61b0, 0x9801).unwrap();
    m.write32(0xe000ed08, 0x6000).unwrap();
    m.run_until(50_000, 1_000_000).unwrap();
    assert_eq!(m.read32(0x2001a000).unwrap(), 0);
    m.run_until(73_000_000, 1_000_000).unwrap();
    assert_eq!(
        words(&mut m, 0x2001a000, 8),
        [0x42, 0x17609d, 3, 0x47702009, 0x47702009, 0, 4, u32::MAX]
    );
    reference(
        &words(&mut m, 0x2001a000, 8),
        "storage-probes.storage-program.bin",
        &[],
    );
    m.write32(0x2001a020, 1).unwrap();
    m.run_until(144_000_000, 1_000_000).unwrap();
    assert_eq!(
        words(&mut m, 0x2001a000, 8),
        [
            0xab,
            0x17609d,
            3,
            0x47702009,
            0x47702009,
            u32::MAX,
            5,
            u32::MAX
        ]
    );
    assert_eq!(
        words(&mut m, 0x2001a024, 7),
        [0x47702009, 9, 0x47702009, 9, 0x47702001, 1, 0x18]
    );
    reference(
        &words(&mut m, 0x2001a000, 8),
        "storage-probes.storage.bin",
        &[],
    );
    reference(
        &words(&mut m, 0x2001a024, 7),
        "storage-probes.storage-cache.bin",
        &[],
    );
}
