# Virtual Daisy

Run Daisy Seed firmware on your computer. Load the ARM ELF you already build,
attach your project's controls, and capture stereo audio and peripheral activity.

The default host is Rust, including the CPU interpreter. It needs no Renode,
.NET or attached board. The previous Renode host remains available for comparisons.

## Try it

Install [Rust](https://rustup.rs/), then:

```sh
git clone https://github.com/UnitedCode/virtual-daisy.git
cd virtual-daisy
cargo run --release --locked -- demo --board rev5 --out results/demo
```

Expected: **`ARM memory check passed.`** This runs a small ARM program, writes a
completion marker, and saves a report. It does not generate audio.

## Run your firmware

Build the firmware in its own project, then give Virtual Daisy its ELF:

```sh
cargo run --release --locked -- emulate \
  --elf /path/to/firmware.elf --board rev5 --out results/capture
```

Each run gets a new folder containing the firmware/hash, execution report,
serial timestamps, and stereo WAV when the firmware produces audio.
Failed executions keep a diagnostic report; previous captures are preserved.
The default duration is one virtual second. Use `--duration-ms 100` for a shorter
run, or `--input recording.wav` to supply serial PCM input.

Choose the physical revision your firmware targets:

| Profile | Codec | SAI input / output |
| --- | --- | --- |
| `rev4` | AK4556 | B / A |
| `rev5` | WM8731 | A / B |
| `rev7` | PCM3060 | B / A |

Seed2 DFM and Seed3 are not supported.

**Extra hardware needs a carrier.** A bare Seed has no screen, keypad or pots.
Attach those devices through the Rust API; see [using the SDK](docs/rust-host.md).
The CLI does not silently acknowledge missing I²C devices.

## What is checked

The same ARM guests run on Rust and the pinned Renode reference backend.
They exercise CPU faults, MPU permissions, FPU state, cache views, QSPI/flash
errors, DMA responses, SAI interrupts and exception frame handling.
Board tests cover revision straps and stereo codec routing.

This is a functional emulator. CPU pipeline timing, electrical behavior and
complete converter filter responses have not been matched against physical
boards. See the [parity record](docs/rust-parity.md) for the exact checks,
intentional reference differences and remaining limits.

## Find the next step

| I want to… | Read |
| --- | --- |
| Attach controls or use the Rust API | [Rust host guide](docs/rust-host.md) |
| Check supported behavior and evidence | [Parity record](docs/rust-parity.md) |
| Use the old backend or run reference probes | [Renode runner](docs/reference-runner.md) |
| Understand the board models and sources | [Conformance notes](docs/conformance.md) |
| Run checks or contribute a fix | [Contributing](CONTRIBUTING.md) |

Virtual Daisy's own code is [MIT licensed](LICENSE). The vendored CPU keeps its
[Apache-2.0 license and attribution](cpu/UPSTREAM.md); see [NOTICE](NOTICE).

Started in [Synthphone E](https://github.com/UnitedCode/synth-phone-e-v2-rust).
The [Virtual Synthphone app](https://github.com/UnitedCode/virtual-synthphone-e)
is a separate consumer of this SDK.

<a href="https://0rwx.com/"><img src="https://0rwx.com/static/AI-X-dark.png" alt="0rwx X: AI made" width="100"></a>
