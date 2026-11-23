# Contributing

Start with a small failing firmware or register-level example. Include the board
revision, firmware SHA256, input/stimulus and the expected result. A manual section
or physical capture makes a useful oracle; a second copy of the implementation
does not.

## Run the checks

Tested locally with Rust 1.97.1 on macOS arm64. The Rust host does not require
Renode or .NET.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked

# The CPU is a separate workspace.
cargo test --locked --manifest-path cpu/Cargo.toml \
  --no-default-features --features armv7em,fpv5-d16

# Executed ARM firmware tests.
rustup target add thumbv7em-none-eabihf
cargo test --locked --test arm_guests -- --ignored
```

To compare fresh Renode captures, run the reference `cpu` suite, then set
`VIRTUAL_DAISY_REFERENCE` to its output directory when running `arm_guests`.
See [the parity record](docs/rust-parity.md) for comparison rules and evidence.

Use `CARGO_TARGET_DIR` to share a compatible existing build cache. On hosts that
reuse native cdylib and test artifacts, `CARGO_PROFILE_RELEASE_LTO=false` avoids
mixing incompatible link-time optimization artifacts. Do not delete another
checkout's active build output.

## Where changes belong

| Area | Location |
| --- | --- |
| CPU decoder/interpreter | `cpu/` — keep Apache-2.0 attribution |
| Physical board models and shared bus/cache behavior | `src/lib.rs`, `src/clock.rs`, `src/memory.rs`, `src/bus.rs` |
| Rust host and carrier contracts | `src/machine/` |
| Renode's required API adapters | `bridge/` |
| Firmware probes | `guests/`, `tests/arm_guests.rs` |
| Instrument-specific controls and UI | The consumer project |

Write new implementation and tooling in Rust. Keep the default setup free of
host-specific paths. Avoid invented timing constants: label functional service
scenarios separately from measured device timing.

Changes to observable behavior need a regression check. For parity fixes, run
the same guest against both hosts and record intentional differences supported
by the hardware documentation. Preserve the actual input ELF and hashes when
collecting evidence. Passing emulator tests does not establish physical parity.

Contributions to Virtual Daisy's own code are MIT licensed; changes in `cpu/`
retain its Apache-2.0 license. See [NOTICE](NOTICE).
