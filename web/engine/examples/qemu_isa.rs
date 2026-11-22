//! Generate an independent ARM QEMU oracle for the browser CPU's float path.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cpu/fixtures");
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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("QEMU reference guest exceeded ten seconds");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert!(status.success());
    let hex = fs::read_to_string(&capture).unwrap();
    let mut bytes = Vec::new();
    for line in hex.lines() {
        assert_eq!(line.len(), 40);
        for pair in line.as_bytes().chunks_exact(2) {
            bytes.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
        }
    }
    assert_eq!(bytes.len(), 210 * 4 * 5 * 4);
    fs::write(&output, &bytes).unwrap();
    fs::remove_file(&elf).unwrap();
    fs::remove_file(&capture).unwrap();
    println!("840 QEMU ARM-ISA reference cases; {} bytes", bytes.len());
}
