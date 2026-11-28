# Use the Rust host

## Load a Seed

Virtual Daisy loads little-endian ARM ELF32 executables. Load segments use their
physical addresses, preserving flash load addresses for initialized RAM data.
The default vector table is `0x08000000`; `Machine::from_elf_at` supports relocated
tables for probes and other firmware layouts. Raw images use `from_binary`.

```rust,no_run
use virtual_daisy::{board::Board, machine::Machine};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let firmware = std::fs::read("firmware.elf")?;
let mut seed = Machine::from_elf(Board::Rev5, &firmware)?;
seed.run_until(100_000_000, 5_000_000)?; // absolute ns, finite scheduler budget
let stereo = seed.take_audio();
let transmitted = seed.take_serial(); // (completed-frame time in ns, byte)
# Ok(())
# }
```

For an existing repository, add Virtual Daisy as a path dependency during local
development. Pin the Git revision or a release tag in shared projects:

```toml
[dependencies]
virtual-daisy = { git = "https://github.com/UnitedCode/virtual-daisy", tag = "v0.2.49" }
```

`rust-host` is enabled by default. Projects using only the peripheral models or
native FFI may set `default-features = false` to omit the interpreter.

To keep firmware settings between sessions, save `nonvolatile_image` and restore
it before running the next machine. This is a sparse, versioned `VDNOR001`
snapshot, not a raw flash dump. It retains programmed-cell history and protection
bits. The CLI accepts `--qspi previous.vdnor` and saves `qspi.vdnor` when firmware
changes the external flash.

## Attach your hardware

Implement `machine::Carrier`, then call `Machine::attach` before execution.
Only override the methods your carrier uses:

| Method | Contract |
| --- | --- |
| `pins` | GPIO ports A–K: `(driven_mask, high_levels)` for each port. Cleared mask bits release contacts previously owned by this carrier. |
| `i2c_write` | Completed master writes; bus numbers 1/2 and seven-bit address. Model the addressed device or return an error. |
| `adc_input` | Single-ended input in microvolts, 0–3,300,000; includes current GPIO outputs and virtual time. |
| `next_event` | Absolute time of the next input change, so sleeping firmware can wake at that boundary. |

Defaults leave GPIO disconnected, ADC at zero and I²C devices absent. Rev5's
WM8731 on I²C2/address `0x1a` belongs to the Seed, not your carrier.

The [recording example](../examples/record_i2c.rs) shows the attachment pattern:

```sh
cargo run --release --locked --example record_i2c -- firmware.elf rev5 1 3c 100
```

This explicitly acknowledges and records writes to one address. It is a recording
sink, not an SSD1306 display model. Replace its callback with the behavior your
project needs. For example, the separate Synthphone app owns its OLED and keypad.

## Audio and serial

`input` queues stereo frames at the serial PCM boundary; no host microphone opens.
Use `input_at_rate` for recordings timestamped at another source rate. It performs
linear interpolation onto the actual SAI clock, retaining phase across refills.
Choose the mode before the first frame is consumed; don't mix modes or change
the source rate during a run. Queued samples and interpolation phase survive
SYSRESETREQ, just as the external signal and board time do.
Drain `take_audio` regularly during long runs. It returns **both channels** as
normalized `f32` samples. Codec power, input mute, DAC mute and WM8731 channel swap
still apply. Firmware must configure its codec and SAI clocks first.

The CLI accepts mono or stereo integer/float WAVs with at most two million
samples. Mono is copied to both channels. It rejects non-finite/out-of-range
samples. By default it rejects sample-rate mismatches. Pass `--resample-input`
to interpolate from the WAV's declared source rate; reports record that choice.
This is a file-input conversion, not a converter filter or analog model. WAV headers use an
integer rate; `report.txt` preserves the exact rational clock rate.
One WAV uses one sample rate. If captured frames span more than one rate, the
CLI saves a diagnostic report and omits the WAV, even if the original rate has
already returned. `Machine::audio_capture_stats()` keeps frame and FIFO error
counts across drains, SAI resets and board resets. Clearing a firmware status
flag does not erase its count, and resetting SAI preserves undrained audio.

`memory_report` also records when the modeled serial clock first ran and emitted
its first frame, its total elapsed nanoseconds and generated frame count. The
reference writes the same `sai_clock_*` fields in its WAV clock report. These
observations start again when the clock model resets. `NOT_OBSERVED` means no
running clock or frame has been seen. The first-frame timestamp is the integer
nanosecond ceiling of the model's rational edge; the running timestamp names the
beginning of its first active advance interval. Adapter scheduling still limits
when configuration changes are observed. These fields describe the model, not
measured physical startup latency.

`receive_serial` requires configured USART1 RX. Bytes arrive at the programmed
baud rate, including FIFO limits and overrun. TX observations are timestamped at
completion of the full 8N1 frame, rather than at the TDR write.

## Run boundaries and errors

`run_until` takes an absolute time and a finite scheduler-step budget. Waits stop
at board events and SysTick deadlines. An instruction can finish just after the
requested boundary; this is not a wall-clock or real-time guarantee.

`read32`/`write32` expose debugger-style physical memory and MMIO. They bypass
CPU cache views. Guest instructions use the modeled instruction/data views.
`pend_interrupt`, `request_dma` and `inject_contention` support explicit fixtures;
they do not stand in for a functioning attached device.

A keyed AIRCR `SYSRESETREQ` reboots the CPU and resets its board controllers.
RAM, external flash, queued external serial arrivals, carrier state and completed
output observations survive. Time and the overall step budget stay monotonic.

Unsupported model behavior returns an error with address and PC. Documented
hardware bus/MPU faults go to firmware handlers. Cortex-M lockup stops the host.
A finite run that ends in an application's fault handler is not a successful
application test: check its completion markers, output and reported fault state.

## Functional browser execution

`Machine::from_elf_functional(board, elf)` is an explicit fast mode for immutable
on-chip flash and DTCM-based interrupt firmware. It executes the original ARM
instructions with cached decoding, direct flash/DTCM storage, and the same
codec, GPIO, clock and timer models. Board service occurs at MMIO, carrier and
peripheral deadlines rather than on every ordinary instruction.

Use this mode for responsive virtual instruments. It is not a substitute for
the default timed runtime's memory-bus, flash/cache, MPU or DMA checks. Flash
programming and DMA requests are rejected; instruction/exception bus delays are
omitted. Audio FIFO batches, codec routing, GPIO contacts, interrupt semantics,
finite step budgets and keyed reset remain real firmware behavior. Functional
reset retains DTCM and time. `from_elf` retains its existing detailed behavior.

The separate Virtual AnsaLoop consumer compares this mode with the timed runtime
and runs the unchanged tape firmware for live recording, playback and cassette
exchange. Synthetic passes remain distinct from hardware validation.

## Troubleshooting

| Message or symptom | Check |
| --- | --- |
| Invalid reset vector | Build for the populated 128 KiB flash, or select the correct relocated table through the API. |
| No I²C device attached | Attach your carrier. The CLI models the Seed's codec, not external displays. |
| No audio | Check codec activation/power, physical A/B directions, RCC gate and SAI enable. The demo intentionally produces none. |
| Codec clocks disagree | Check PLL source/dividers, WM8731 sampling mode, MCLK/LRC ratio and board revision. |
| Step budget exhausted | Inspect the final PC and firmware polling condition; increase the budget only after understanding the loop. |
| Unsupported register/mode | Reduce to an ARM guest and open an issue with the access, firmware hash and expected register behavior. |

API documentation: `cargo doc --no-deps --open`.


## Event-driven firmware

`Machine::signal_event()` supplies a digital event to the core. Use it when an
attached device changes an event input; it wakes WFE and leaves WFI asleep.
`Machine::pend_interrupt(n)` supplies an external IRQ pending edge, including
while disabled in the NVIC. Firmware controls enabling, masking and SEVONPEND.
The runner advances board time and peripherals while the core sleeps. These
APIs do not simulate a board's electrical event pin or low-power supply state.
