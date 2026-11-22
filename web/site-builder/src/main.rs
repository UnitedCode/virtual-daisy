//! Build a self-contained static site. No server-side runtime is needed.
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
mod serve;
fn run(command: &mut Command) -> Result<(), Box<dyn std::error::Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("Build command failed with {status}").into());
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../engine")
        .canonicalize()?;
    let dist = engine.parent().unwrap().parent().unwrap().join("dist");
    if env::args().nth(1).as_deref() == Some("serve") {
        serve::serve(&dist)?;
        return Ok(());
    }
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| engine.join("target"));
    let target = if target.is_absolute() {
        target
    } else {
        env::current_dir()?.join(target)
    };
    run(Command::new("cargo")
        .current_dir(&engine)
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_INCREMENTAL", "0"))?;
    fs::create_dir_all(&dist)?;
    run(Command::new("wasm-bindgen")
        .arg(target.join("wasm32-unknown-unknown/release/virtual_daisy_web.wasm"))
        .args(["--target", "web", "--out-name", "engine", "--out-dir"])
        .arg(&dist))?;
    fs::copy(engine.join("index.html"), dist.join("index.html"))?;
    fs::copy(
        engine.join("../firmware/synthphone.bin"),
        dist.join("synthphone.bin"),
    )?;
    println!("Static browser site built at {}", dist.display());
    Ok(())
}
