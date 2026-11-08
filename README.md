# Virtual Daisy

A virtual Daisy Seed for running firmware without the board plugged in.

Rust models handle the codec, SAI, DMA, clocks, GPIO, flash, SDRAM and Cortex-M7
memory behavior. Renode runs the ARM instructions through a small C# API bridge.
The runner loads an existing ELF, records audio and peripheral counters, and
keeps the firmware unchanged.

This started in [Synthphone E](https://github.com/UnitedCode/synth-phone-e-v2-rust).
It lives here so other Daisy projects can use the same board models.

## Running firmware

Install Rust, a .NET 8 SDK, CMake, a C compiler, and the portable
[Renode 1.17.0 runtime](https://github.com/renode/renode/releases/tag/v1.17.0).
ARM conformance probes also need `rustup target add thumbv7em-none-eabihf`.
Build your firmware in its own project, then run:

```sh
cargo run --release --locked -- capture /path/to/renode-runtime results/capture \
  --elf /path/to/firmware.elf --board rev5
```

The runtime directory must contain `Infrastructure.dll`, `platform-lib/` and
`platforms/cpus/stm32h7.repl`. Set `VIRTUAL_DAISY_DOTNET` if the SDK executable
isn't named `dotnet`. The first run downloads pinned, checksum-verified CPU engine
sources and builds the native adapter. Later runs can reuse
`VIRTUAL_DAISY_CORE_CACHE`.

`--elf` takes precedence over `VIRTUAL_DAISY_ELF`. There is no assumed firmware
checkout, Cargo package name or parent directory. Firmware must use a flash
vector table at `0x08000000`. The Renode Monitor and Python execution are disabled.

| Board | Codec | SAI receive / transmit |
| --- | --- | --- |
| `rev4` | AK4556 | B / A |
| `rev5` | WM8731 | A / B |
| `rev7` | PCM3060 | B / A |

Use `cargo run -- profiles` to inspect the wiring. Seed2 DFM and Seed3 are not
supported.

`capture` runs one virtual second by default. `VIRTUAL_DAISY_AUDIO_STEPS` accepts
1–1000 steps of 10 ms. Input defaults to silence; append a mono or stereo 48014 Hz
WAV to replay up to 1.1 seconds. Captures contain WAV audio, optional RTT output,
register counters, runtime logs, and the copied ELF with its SHA256. A successful
capture means execution completed without an unexpected runtime warning; it does
not grade the application's behavior or require Synthphone startup messages.

For a Rust project, the library can also be used directly:

```toml
[dependencies]
virtual-daisy = { git = "https://github.com/UnitedCode/virtual-daisy", tag = "v0.1.1" }
```

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

`cpu`, `storage`, `timing` and `probe` run standalone ARM conformance programs
and their negative controls. They still take an ELF for artifact provenance, but
do not test that application's audio. `run`, `startup`, `settings`, `levels` and
`autotune` retain the Synthphone regression fixtures and their application-specific
expectations. See the [Synthphone guide](docs/synthphone.md) for those commands.

The writeback and interrupt probes erase their temporary machine's internal flash
through the modeled controller before installing their fixtures. This keeps their
scratch area independent of the supplied firmware's size. ELF files stay unchanged;
production audio runs load the original firmware into a separate machine.

The models are functional service scenarios. Instruction timing is not cycle
accurate, analog behavior is incomplete, and hardware equivalence is unverified.
See [conformance and remaining gaps](docs/conformance.md) before interpreting a
simulated pass as a hardware result.

The engine downloads retain the upstream Renode infrastructure and tlib licenses.
This repository does not bundle their binaries or change their licensing.
