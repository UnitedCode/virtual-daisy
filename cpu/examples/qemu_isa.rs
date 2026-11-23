//! Generate independent QEMU fixtures for ARM floating-point, DSP and PSR paths.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .expect("output .bin");
    let elf = output.with_extension("elf");
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
            root.join("isa_oracle.ld").display()
        ))
        .arg(root.join("isa_oracle.rs"))
        .arg("-o")
        .arg(&elf)
        .status()
        .unwrap();
    assert!(status.success());
    let capture = output.with_extension("hex");
    let mut child = Command::new("qemu-system-arm")
        .args([
            "-M",
            "mps2-an500",
            "-nographic",
            "-semihosting-config",
            "enable=on,target=native",
            "-kernel",
        ])
        .arg(&elf)
        .stderr(Stdio::from(fs::File::create(&capture).unwrap()))
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("QEMU reference guest exceeded sixty seconds");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert!(status.success());
    let hex = fs::read_to_string(&capture).unwrap();
    let mut bytes = Vec::new();
    let mut doubles = Vec::new();
    let mut controlled = Vec::new();
    let mut minmax = Vec::new();
    let mut rounding = Vec::new();
    let mut arithmetic = Vec::new();
    let mut fused = Vec::new();
    let mut precision = Vec::new();
    let mut accumulate = Vec::new();
    let mut half = Vec::new();
    let mut fixed = Vec::new();
    let mut saturation = Vec::new();
    let mut psr = Vec::new();
    let mut qadd = Vec::new();
    for line in hex.lines() {
        assert!(matches!(line.len(), 40 | 48 | 56 | 64 | 72 | 80 | 88 | 104));
        let destination = if line.len() == 48 && line.ends_with("53534154") {
            &mut saturation
        } else if line.len() == 72 && line.ends_with("48414c46") {
            &mut half
        } else if line.len() == 40 {
            &mut bytes
        } else if line.len() == 56 && line.ends_with("51414444") {
            &mut qadd
        } else if line.len() == 56 {
            &mut precision
        } else if line.len() == 64 && line.ends_with("41505352") {
            &mut psr
        } else if line.len() == 64 && line.ends_with("46495844") {
            &mut fixed
        } else if line.len() == 64 {
            &mut doubles
        } else if line.len() == 80 {
            &mut minmax
        } else if line.len() == 104 {
            if line.ends_with("564d4c41") {
                &mut accumulate
            } else {
                &mut fused
            }
        } else if line.len() == 88 {
            &mut arithmetic
        } else if line.len() == 72 {
            &mut rounding
        } else {
            &mut controlled
        };
        for pair in line.as_bytes().as_chunks::<2>().0 {
            destination.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
        }
    }
    assert_eq!(bytes.len(), 210 * 5 * 5 * 4);
    fs::write(&output, &bytes).unwrap();
    assert_eq!(doubles.len(), 344 * 5 * 8 * 4);
    fs::write(output.with_extension("f64.bin"), &doubles).unwrap();
    assert_eq!(controlled.len(), 344 * 7 * 4 * 6 * 4);
    fs::write(output.with_extension("control.bin"), &controlled).unwrap();
    assert_eq!(minmax.len(), 456 * 4 * 2 * 2 * 10 * 4);
    fs::write(output.with_extension("minmax.bin"), &minmax).unwrap();
    assert_eq!(rounding.len(), 39_680 * 9 * 4);
    fs::write(output.with_extension("rounding.bin"), &rounding).unwrap();
    assert_eq!(arithmetic.len(), 61_440 * 11 * 4);
    fs::write(output.with_extension("arithmetic.bin"), &arithmetic).unwrap();
    assert_eq!(fused.len(), 49_152 * 13 * 4);
    fs::write(output.with_extension("fused.bin"), &fused).unwrap();
    assert_eq!(precision.len(), 32_768 * 7 * 4);
    fs::write(output.with_extension("precision.bin"), &precision).unwrap();
    assert_eq!(accumulate.len(), 49_152 * 13 * 4);
    fs::write(output.with_extension("accumulate.bin"), &accumulate).unwrap();
    assert_eq!(half.len(), 331_776 * 9 * 4);
    fs::write(output.with_extension("half.bin"), &half).unwrap();
    assert_eq!(fixed.len(), 401_408 * 8 * 4);
    fs::write(output.with_extension("fixed.bin"), &fixed).unwrap();
    assert_eq!(saturation.len(), 520_192 * 6 * 4);
    fs::write(output.with_extension("saturate.bin"), &saturation).unwrap();
    assert_eq!(psr.len(), 768 * 8 * 4);
    fs::write(output.with_extension("psr.bin"), &psr).unwrap();
    assert_eq!(qadd.len(), 8192 * 7 * 4);
    fs::write(output.with_extension("qadd.bin"), &qadd).unwrap();
    let opcodes = [0xfa82f081, 0xfa82f0a1, 0xfa82f091, 0xfa82f0b1];
    let mut word_source = String::from(
        "// Generated from independent QEMU records by cpu/examples/qemu_isa.rs.\npub const WORD_CASES: [[u32; 5]; 4] = [\n",
    );
    let mut word_cases = 0;
    for record in qadd.as_chunks::<28>().0 {
        let words: [u32; 7] = core::array::from_fn(|i| {
            u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
        });
        if words[1] != 0x80000000 || words[2] != 0x80000000 || words[3] != 0xa00a0000 {
            continue;
        }
        word_source.push_str("    [");
        for word in [
            opcodes[words[0] as usize],
            words[1],
            words[3],
            words[4],
            words[5],
        ] {
            word_source.push_str(&format!("0x{word:08x}, "));
        }
        word_source.push_str("],\n");
        word_cases += 1;
    }
    assert_eq!(word_cases, 4);
    word_source.push_str("];\n");
    fs::write(output.with_extension("qadd-probe.rs"), word_source).unwrap();
    let mut psr_source = String::from(
        "// Generated from independent QEMU records by cpu/examples/qemu_isa.rs.\npub const CASES: [[u32; 7]; 48] = [\n",
    );
    let mut psr_cases = 0;
    for record in psr.as_chunks::<32>().0 {
        let words: [u32; 8] = core::array::from_fn(|i| {
            u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
        });
        if words[2] != 0xa80a0000 || ![0x50050000, 0x01000000].contains(&words[1]) {
            continue;
        }
        psr_source.push_str("    [");
        for word in &words[..7] {
            psr_source.push_str(&format!("0x{word:08x}, "));
        }
        psr_source.push_str("],\n");
        psr_cases += 1;
    }
    assert_eq!(psr_cases, 48);
    psr_source.push_str("];\n");
    fs::write(output.with_extension("psr-probe.rs"), psr_source).unwrap();
    let selected = [
        0xf3010000, 0xf301001f, 0xf30170cf, 0xf32170cf, 0xf3810000, 0xf381001f, 0xf38170cf,
        0xf3a170cf, 0xf3210000, 0xf321000f, 0xf3a10000, 0xf3a1000f,
    ];
    let mut saturation_source = String::from(
        "// Generated from independent QEMU records by cpu/examples/qemu_isa.rs.\npub const CASES: [[u32; 5]; 12] = [\n",
    );
    for opcode in selected {
        let block = saturation
            .chunks_exact(128 * 24)
            .find(|block| u32::from_le_bytes(block[..4].try_into().unwrap()) == opcode)
            .unwrap();
        let half = opcode & 0x002070c0 == 0x00200000;
        let sample = if opcode & (1 << 23) != 0 && !half {
            4
        } else {
            8
        };
        let sticky = usize::from(opcode == 0xf321000f);
        let record = &block[(sample * 4 + sticky) * 24..(sample * 4 + sticky + 1) * 24];
        saturation_source.push_str("    [");
        for word in record[..20].as_chunks::<4>().0 {
            saturation_source.push_str(&format!("0x{:08x}, ", u32::from_le_bytes(*word)));
        }
        saturation_source.push_str("],\n");
    }
    saturation_source.push_str("];\n");
    fs::write(
        output.with_extension("saturate-probe.rs"),
        saturation_source,
    )
    .unwrap();
    let mut cases = Vec::new();
    for block in fixed.chunks_exact(1024 * 32) {
        let opcode = u32::from_le_bytes(block[..4].try_into().unwrap());
        let width = if opcode & (1 << 7) != 0 { 32 } else { 16 };
        let fbits = width - (((opcode & 15) << 1) | ((opcode >> 5) & 1));
        if (width == 16 && ![0, 16].contains(&fbits))
            || (width == 32 && ![1, 19, 32].contains(&fbits))
        {
            continue;
        }
        let sample = if opcode & (1 << 18) == 0 {
            8
        } else if opcode & (1 << 16) != 0 {
            4
        } else {
            5
        };
        let record = &block[sample * 16 * 32..(sample * 16 + 1) * 32];
        let words: [u32; 7] = core::array::from_fn(|i| {
            u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
        });
        cases.push(words);
    }
    assert_eq!(cases.len(), 40);
    let mut source = String::from(
        "// Generated from independent QEMU records by cpu/examples/qemu_isa.rs.\npub const CASES: [[u32; 7]; 40] = [\n",
    );
    for row in cases {
        source.push_str("    [");
        for word in row {
            source.push_str(&format!("0x{word:08x}, "));
        }
        source.push_str("],\n");
    }
    source.push_str("];\n");
    fs::write(output.with_extension("fixed-probe.rs"), source).unwrap();
    fs::remove_file(&elf).unwrap();
    fs::remove_file(&capture).unwrap();
    println!("1050 QEMU ARM-ISA reference cases; {} bytes", bytes.len());
    println!(
        "1720 QEMU binary64 reference cases; {} bytes",
        doubles.len()
    );
    println!(
        "9632 QEMU binary32 control-mode reference cases; {} bytes",
        controlled.len()
    );
    println!(
        "7296 QEMU FPv5 minimum/maximum reference cases; {} bytes",
        minmax.len()
    );
    println!(
        "39680 QEMU FPv5 rounding/conversion cases; {} bytes",
        rounding.len()
    );
    println!(
        "61440 QEMU arithmetic rounding cases; {} bytes",
        arithmetic.len()
    );
    println!("49152 QEMU fused arithmetic cases; {} bytes", fused.len());
    println!(
        "32768 QEMU precision conversion cases; {} bytes",
        precision.len()
    );
    println!(
        "49152 QEMU nonfused accumulate cases; {} bytes",
        accumulate.len()
    );
    println!("331776 QEMU half conversion cases; {} bytes", half.len());
    println!("401408 QEMU fixed conversion cases; {} bytes", fixed.len());
    println!("520192 QEMU saturation cases; {} bytes", saturation.len());
    println!("768 QEMU PSR mask/alias cases; {} bytes", psr.len());
    println!(
        "8192 QEMU saturating word arithmetic cases; {} bytes",
        qadd.len()
    );
}
