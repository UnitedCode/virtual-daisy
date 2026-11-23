//! Record independent Cortex-M7 branch and unallocated-hint results.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("usage: qemu_control NEW_OUTPUT.bin")?,
    );
    let elf = output.with_extension("elf");
    let capture = output.with_extension("hex");
    for path in [&output, &elf, &capture] {
        if path.exists() {
            return Err(format!(
                "{} already exists; choose a new output prefix",
                path.display()
            )
            .into());
        }
    }
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
        .arg(root.join("control_oracle.rs"))
        .arg("-o")
        .arg(&elf)
        .status()?;
    if !status.success() {
        return Err("control guest compilation failed".into());
    }
    let mut child = Command::new("qemu-system-arm")
        .args([
            "-M",
            "mps2-an500",
            "-nographic",
            "-monitor",
            "none",
            "-serial",
            "null",
            "-semihosting-config",
            "enable=on,target=native",
            "-kernel",
        ])
        .arg(&elf)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(fs::File::create(&capture)?))
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("QEMU control guest exceeded sixty seconds".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return Err(format!("QEMU control guest failed; see {}", capture.display()).into());
    }
    let mut bytes = Vec::new();
    for line in fs::read_to_string(&capture)?.lines() {
        let Some(hex) = line.strip_prefix("CTRL:") else {
            continue;
        };
        if hex.len() != 40 {
            return Err("truncated QEMU control record".into());
        }
        for at in (0..40).step_by(2) {
            bytes.push(u8::from_str_radix(&hex[at..at + 2], 16)?);
        }
    }
    if bytes.len() != 9664 * 20 {
        return Err(format!("expected 9664 control records, got {} bytes", bytes.len()).into());
    }
    fs::write(&output, &bytes)?;
    println!(
        "9664 independent QEMU control records; {} bytes",
        bytes.len()
    );
    Ok(())
}
