# Rust host parity

The Rust host runs unchanged ARM firmware against the same board, clock, memory,
cache and bus models used by the Renode host. The CPU is the vendored zmu
interpreter, not a Rust port of Renode.

**Functional comparisons pass for the cases below. Full physical-device or
cycle-for-cycle equivalence has not been established.**

## Compared on 22 November 2026

Reference: Renode 1.17.0 macOS arm64 runtime, with the repository's pinned native
adapter. Infrastructure commit `066a7f13c052215632d469c995c89aea37c573b1`;
tlib commit `167decf9129758829762e582939128c0694e90d6`. Downloads are checksum-checked.
Guests were built with Rust 1.97.1 for `thumbv7em-none-eabihf`, Cortex-M7,
`opt-level=s`. Tests use the same guest sources, linker layout and stimuli.

| Executed firmware | Comparison |
| --- | --- |
| CPU faults and FPU | All 100 result words match for DTCM/shared SRAM stacks and lazy/eager FP saves. Includes divide-by-zero, normal/forced alignment, MPU denied/XN access, NOCP, f64 arithmetic, FP identifiers, scalar/paired VMOV bit transfers and precise fault PCs. |
| SysTick source selection | An ARM guest compares both CLKSOURCE settings against DWT cycles. The reference suite also restores the old core-only behavior as a required negative control. |
| QSPI bus faults / DMA | Five fixtures: disabled mapping, valid DMA completion, controller disable, abort and aperture range error. Checks precise data/execute faults, escalation, flash key errors, TEIF/TCIF, EN, NDTR and interrupt delivery. |
| Demand instruction fetch | Valid prefixes execute; Thumb32 crossing the QSPI limit faults at the instruction's start; the faulting store does not occur; cached prefixes remain executable after controller disable. |
| SAI | Reset values, FIFO level, mute detect/clear and IRQ87 delivery. |
| SDRAM | Rows/banks, geometry BusFault, refresh error/clear and IRQ48. Activation, precharge, refresh and FIFO paths are exercised. |
| Vector faults | Ordinary, cached HardFault vector and enabled configurable faults. The original SRAM frame is reused and the original IRQ remains pending. |
| Interrupt transitions | Late arrival, equal priorities, nested preemption, tail chaining, interrupted return, lazy FP, PSP and FP+PSP. Checks handler order, frame addresses, EXC_RETURN, restored registers and exact frame-store counts. |
| Generated-code coherence | The original Thumb fixture checks dirty CPU code, DMA writes, separate I/D views, flash/RAM conflict eviction and explicit maintenance. All 15 result words match. |
| Internal flash | Erase/program completion, resumable STM/STRD writes, stale execution until invalidation and one-second modeled erase service. All 10 result words match. |
| QSPI persistence | Program/sector erase, controller-reset retention, cached data/execution, WFI/IRQ92 and timeout rescheduling after a kernel-clock change. All programmed, erased and cache result words match. |
| Writeback and interrupted loads | The original DSB, busy-vector and flash/shared-bus preemption programs preserve their frames and restart abandoned loads. DWT values are bounded independently; other result words match. |

These are assertions on firmware-written results, not just host counters or a
boot message. The Renode suite also runs its existing negative controls.
The Rust tests keep independent register expectations when no reference capture
directory is supplied.

Additional Rust tests cover all three revision straps, stereo ADC/DAC routing,
WM8731 DAC-only channel swap, RCC gates/resets, baud-paced USART1 TX/RX and overrun,
TIM2 update, SysTick clock selection, NVIC readbacks, keyed system reset, persistent
flash snapshots, malformed images and finite CLI budgets.

TIM2 regressions cover UDIS/URS, software updates, buffered PSC/ARR changes,
one-pulse stopping, a zero reload and UIF remapping. Batched clock advances must
agree with split advances across a pending prescaler change. The expectations
follow [RM0433, general-purpose timers](https://www.st.com/resource/en/reference_manual/dm00314099-stm32h742-stm32h743753-and-stm32h750-value-line-advanced-armbased-32bit-mcus-stmicroelectronics.pdf),
CR1/EGR/PSC/ARR. CPU checks also cover every valid signed-bitfield width/offset
for representative positive/negative inputs, and reject invalid widths without
panicking. VMOV follows the [Armv7-M architecture manual](https://documentation-service.arm.com/static/606dc36485368c4c2b1bf62f),
A7.7.241–244; scalar words preserve the other half and paired moves allow odd S
registers while rejecting a pair beginning at S31.

## Deliberate differences from the reference

The hardware manual takes precedence where the previous model is incomplete:

| Field | Rust behavior and comparison rule |
| --- | --- |
| ICSR pending fields | Reports enabled pending exception number and interrupt-pending state, with BASEPRI/FAULTMASK filtering and no PRIMASK filtering for VECTPENDING. The reference captures omit these fields. Transition/vector comparisons mask them; direct MMIO checks cover them. |
| FPCCR.THREAD | Retains that a lazy FP frame was allocated in Thread mode. Renode's transition capture clears this bit. The comparison excludes this bit and separately requires the documented value. |
| Nested interrupt's stacked PC | A timed arrival can stop at a different instruction of the same polling loop because CPU instruction timing differs. The test requires valid probe code, the interrupted handler's IPSR, restored state and the expected handler order; it does not require the same polling-loop instruction. |
| DWT probe timestamps | Writeback/vector and interrupted-load probes compare architectural results and independently require the specified before/after service boundaries. Their absolute DWT samples are excluded from equality because instruction costs differ. |

The first two follow ST's [PM0253 programming manual, Rev6](https://www.st.com/resource/en/programming_manual/pm0253-stm32f7-series-and-stm32h7-series-cortexm7-processor-programming-manual-stmicroelectronics.pdf),
ICSR Table53 and FPCCR Table96. The NVIC clear-register reads use the same
positive enabled/pending bits as the set-register reads; writing zero has no effect.

## Real firmware check

The Synthphone ELF with SHA256
`a17bf43410c1acc8b446d1cfb704ac9851aca59861077a7f8fab1a28bb14aafa`
ran for 100 virtual ms with the explicit I²C recording carrier. An early Rust run
recorded 119 external I²C packets and produced 4,193 stereo frames. Renode produced
4,731 frames over the same interval. Both used the same unmodified input ELF.

This confirms execution and audio activity, not matching startup timing, audio
quality or the complete Synthphone UI. The external I²C recording sink acknowledges
one chosen address; it does not validate an OLED's pixels. The separate instrument
application owns that device model and its end-to-end tests.

A follow-up controlled capture used the same frozen ELF
`6282df022fa18ec4f9ce22ab33bc3f4c2a66e82df7a9a4ac89d7d52464866ec6`
before and after correcting the Renode runner's fixed 480 MIPS budget. It now
follows the modeled RCC core clock, starting at 64 MHz. At 100 ms the reference
changed from 4,731 to 4,728 frames, with no FIFO overrun/underrun or CPU fault.
This corrects a reference pacing error but explains only three frames of the
startup difference. Renode still budgets instructions rather than M7 pipeline
cycles; the Rust host retains instruction costs and coupled bus waits.

The next correction restored SysTick's selectable core/8 source in the Renode
adapter. Its upstream NVIC model forced CLKSOURCE high and ignored the firmware's
external-clock request. With the same 200 ms ELF/input capture, Rev5 changed from
9,547 to 9,032 frames; Rust produced 9,011. Both report zero FIFO errors. This
reduces the startup difference from 536 to 21 frames, without changing firmware
or tuning a delay to match the result. Waveform equality remains a separate check.

## Supported scope and remaining limits

| Area | Current scope |
| --- | --- |
| Seed revisions | Rev4/AK4556, Rev5/WM8731 and Rev7/PCM3060. No Seed2 DFM or Seed3 profile. |
| CPU timing | Interpreter cycle counts and coupled functional bus service. No calibrated M7 pipeline, dual issue, speculation, store-buffer overlap or exception-entry latency. Startup frame counts and arbitrary timed interrupt PCs can differ. |
| SysTick silicon scope | Selectable core and core/8 clocks, matching revision V. Earlier revision-Y behavior without the /8 divider is not a selectable silicon profile; see [ES0392](https://www.st.com/resource/en/errata_sheet/es0392-stm32h747xibixg-device-errata-stmicroelectronics.pdf), SysTick external-clock erratum. Board codec revisions do not identify the die revision. |
| Audio | Stereo serial PCM, codec routing/power/mute and validated clocks. Exact rational rate is recorded. Converter analog noise, clipping curves and complete filter coefficients/rounding are not modeled. |
| Memory | Populated H750 memories, shared cache/fabric, FMC SDRAM, internal flash and external IS25LP064A controller/chip models. Retention, electrical signal integrity and physical contention timing are unverified. |
| General peripherals | GPIO A–K; DMA1 word transfers for SAI/explicit requests; USART1 8N1/oversampling16/prescaler1; TIM2 internal up-counter/update; blocking I²C1/2 master writes; single-channel software ADC1/2 conversions. Other modes/peripherals require models. |
| Reset | Keyed SYSRESETREQ and modeled RCC controller resets. Reset transient timing, brownout and the boot ROM are not modeled. |
| Carrier hardware | Supplied by each project through `Carrier`; no implicit screen, keypad, pots, USB or Bluetooth device. |
| Physical acceptance | No independent real-board trace/audio captures have been compared. Reports retain `hardware_equivalence=UNVERIFIED`. |

Unsupported modeled configurations produce errors. This is not a promise that
every possible STM32H750 access is implemented. See [conformance notes](conformance.md)
for model-level details and primary sources.

## Reproduce

```sh
rustup target add thumbv7em-none-eabihf
cargo test --locked --test arm_guests -- --ignored
```

For a fresh cross-backend comparison, run the reference `cpu` command with an
explicit runtime, output directory, firmware ELF and board profile (see
[reference setup](reference-runner.md)), then:

```sh
VIRTUAL_DAISY_REFERENCE=/path/to/reference-results \
  cargo test --locked --test arm_guests -- --ignored
```

The original hand-generated Thumb programs need no ARM compiler:

```sh
cargo test --locked --test rust_probes
```

Use the reference runner's `probe` command for those captures, then set
`VIRTUAL_DAISY_REFERENCE` for both `arm_guests` and `rust_probes`. A `cpu`-only
reference directory does not contain the legacy memory/storage captures.

An optional Synthphone consumer check feeds deterministic serial PCM on Rev4,
Rev5 and Rev7 and checks both output channels, fault flags and FIFO status:

```sh
VIRTUAL_DAISY_FIRMWARE=/path/to/synthphone.elf \
VIRTUAL_DAISY_AUDIO_OUT=/path/to/new-capture-directory \
  cargo test --release --locked --test firmware_audio -- --ignored --nocapture
```

The output directory is optional and must be new. When provided, it retains the
ELF/hash, input WAV, each board's output WAV and memory/status reports. This uses
an explicit I2C display sink; it does not inspect pixels or open host audio.
The test is ignored in ordinary CI because the firmware belongs to its consumer
project. It is a finite capture, not a real-time host-performance test.
Set `VIRTUAL_DAISY_AUDIO_BOARD=rev5` to check just one supported board.

The recording is timestamped at 48,014 Hz. Both backends interpolate onto the
derived SAI clock; the Rust `input_at_rate` queue is checked sample-for-sample
against the original reference interpolator, including fractional clocks,
stereo data, empty input and chunked refills. `input` still means one queued
serial frame per SAI frame. Treating these two input modes as interchangeable
would introduce a pitch/phase difference before the firmware processes anything.

Compare saved stereo float32 WAVs without playing audio:

```sh
cargo run --release --locked --example compare_audio -- rust.wav reference.wav
```

The report preserves different frame counts and the unshifted error. A separate
offset search diagnoses constant delays; it does not establish timing parity.
A single tone cannot identify a unique delay because its waveform repeats.

The reference directory must come from this guest revision and compiler build:
exact instruction addresses are intentionally compared. Keep its copied ELF,
hashes, `.host.log`, result binaries and model reports together.
