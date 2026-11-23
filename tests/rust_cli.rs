#![cfg(feature = "rust-host")]
use std::{fs, process::Command};
#[test]
fn demo_needs_no_dotnet_and_preserves_both_success_and_failure_evidence() {
    let dir = std::env::temp_dir().join(format!("daisy-cli with spaces-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    for failed in [false, true] {
        let output = Command::new(env!("CARGO_BIN_EXE_virtual-daisy"))
            .env("VIRTUAL_DAISY_DOTNET", "/must-not-be-used")
            .args(["demo", "--board", "rev5", "--out"])
            .arg(&dir)
            .args(if failed {
                vec!["--max-steps", "1"]
            } else {
                vec![]
            })
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            !failed,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let runs: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|p| p.unwrap().path())
        .collect();
    assert_eq!(runs.len(), 2);
    let reports: Vec<_> = runs
        .iter()
        .map(|p| fs::read_to_string(p.join("report.txt")).unwrap())
        .collect();
    assert_eq!(
        reports
            .iter()
            .filter(|s| s.contains("status=completed"))
            .count(),
        1
    );
    assert_eq!(
        reports
            .iter()
            .filter(|s| s.contains("status=failed"))
            .count(),
        1
    );
    for p in runs {
        assert!(p.join("firmware.bin").exists());
        assert!(p.join("memory.txt").exists());
    }
    fs::remove_dir_all(dir).unwrap();
}
