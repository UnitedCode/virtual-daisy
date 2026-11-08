use std::{fs, process::Command};

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_virtual-daisy"))
        .env_remove("VIRTUAL_DAISY_ELF")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn firmware_must_be_supplied_instead_of_building_a_parent_project() {
    let result = run(&[
        "capture",
        "/missing/runtime",
        "/missing/output",
        "--board",
        "rev5",
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Supply --elf PATH"));
}

#[test]
fn explicit_elf_overrides_the_environment_and_rejects_non_elf_input() {
    let path =
        std::env::temp_dir().join(format!("virtual-daisy-invalid-{}.elf", std::process::id()));
    fs::write(&path, b"not firmware").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_virtual-daisy"))
        .env("VIRTUAL_DAISY_ELF", "/missing/firmware.elf")
        .args([
            "capture",
            "/missing/runtime",
            "/missing/output",
            "--board",
            "rev5",
            "--elf",
        ])
        .arg(&path)
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Firmware is not an ELF"));
}

#[test]
fn audio_measurements_work_without_the_synthphone_dsp_crate() {
    let samples = virtual_daisy::audio::tone(220.0, 0.5, &[(1, 0.2)]);
    let metrics = virtual_daisy::audio::metrics(&samples);
    assert!((metrics.rms - 0.2 / 2_f64.sqrt()).abs() < 0.001);
    assert_eq!(metrics.non_finite, 0);
    assert!((virtual_daisy::audio::estimate_pitch(&samples).unwrap() - 220.0).abs() < 0.1);
    assert!(virtual_daisy::audio::estimate_pitch(&vec![0.0; 48014]).is_none());
}
