//! Generate an independent ARM QEMU oracle for the browser CPU's float path.
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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("QEMU reference guest exceeded thirty seconds");
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
    for line in hex.lines() {
        assert!(matches!(line.len(), 40 | 48 | 56 | 64 | 72 | 80 | 88 | 104));
        let destination = if line.len() == 72 && line.ends_with("48414c46") {
            &mut half
        } else if line.len() == 40 {
            &mut bytes
        } else if line.len() == 56 {
            &mut precision
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
}
