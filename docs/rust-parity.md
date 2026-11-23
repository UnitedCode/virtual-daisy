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

Breakpoint regressions check HardFault/lockup delivery, the debug status bits,
DebugMonitor vector 12, priority masks/grouping and software monitor pending.
Without an explicitly installed semihosting handler, `BKPT #0xab` has the same
architectural behavior as other breakpoints. Unsupported semihosting operations
return to breakpoint fault delivery rather than panicking the host. These checks
follow the [Armv7-M manual](https://documentation-service.arm.com/static/606dc36485368c4c2b1bf62f),
B1.5.6, C1.5 and C1.6. They are independent CPU tests, not Renode comparisons.
Halting debug, hardware breakpoint comparators and monitor single stepping are
not implemented.

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

The frozen Synthphone ELF has SHA256
`6282df022fa18ec4f9ce22ab33bc3f4c2a66e82df7a9a4ac89d7d52464866ec6`.
Each run executes 200 virtual ms with the same 220 Hz, 0.15-amplitude recording
at 48,014 Hz, interpolated onto the derived serial clock. The display carrier
acknowledges I²C address `0x3c`; it does not validate OLED pixels or the complete UI.

| Board | Rust frames | Renode frames | First signal frame, Rust / Renode | Unshifted sample RMS error |
| --- | ---: | ---: | ---: | ---: |
| Rev4 | 9,497 | 9,515 | 536 / 487 | 0.109911 |
| Rev5 | 9,011 | 9,032 | 537 / 478 | 0.129409 |
| Rev7 | 9,497 | 9,515 | 536 / 487 | 0.109911 |

All six runs report zero CPU faults and zero SAI FIFO overruns/underruns.
Both Rust output channels stay finite and within full scale. The final 4,096
samples have RMS above 0.005 and no silent 256-sample block.
**These checks pass; the waveforms do not match at the same serial-frame index.**
Frame counts include startup, rather than discarding the differing beginning.

Two reference corrections matter for interpreting this result:

- Renode now follows the modeled RCC core clock, starting at 64 MHz, rather than
  running at a fixed 480 MIPS. This changed the Rev5 100 ms count from 4,731 to
  4,728 frames, explaining only three frames of the original gap.
- Its NVIC adapter now honors SysTick's selectable core/8 source. The upstream
  model forced CLKSOURCE high, ignoring this firmware's external-clock request.
  At 200 ms, correcting that changed Rev5 from 9,547 to 9,032 frames and reduced
  the Rust/reference startup gap from 536 to 21 frames. An independent ARM clock
  test and an old-model negative control guard the correction.

A separate, unshipped diagnostic forced ordinary Rust instruction retirement to
one cycle while retaining bus waits and exception costs. With the same Rev5 ELF
and input, the unshifted RMS error fell from 0.129409 to 0.007027; the first signal
moved from frame 537 to 488, versus reference frame 478. This establishes that
instruction pacing materially affects this comparison. It does not explain every
remaining difference or validate a one-cycle Cortex-M7 model. Production retains
its instruction costs. Neither backend models the full M7 pipeline.

No microphone, audio playback or physical device was used. Audio quality,
complete UI behavior and physical-device timing remain unverified.

### Newer compiler's fractional-clock regression

The current production ELF, built with Rust 1.100, has SHA256
`dcb44da90f428f7b7cd611acc1933e62771ff84e8622b21034ae48a8fa3f10ca`.
Its float-to-integer casts use FPv5 `VMAXNM`/`VMINNM`. Version 0.2.1 lacked
these instructions, and an incorrect generic MCR2 decode mask silently accepted
them as no-ops. That made the PLL fractional numerator zero: Rust reported
48,160.173 Hz while Renode programmed FRACN=827 and reported 48,187.487 Hz.

Version 0.2.2 implements both precisions and corrects the decode mask. The same
unmodified ELF now writes `PLL3FRACR=0x19d8` and derives exactly
`3256703125/67584` Hz on Rust, matching the reference. The earlier Rust 1.97.1
firmware comparisons above remain valid; their cast sequence did not use the
missing instructions.

An independent QEMU Cortex-M7 program supplies 7,296 bit-exact min/max result
and FPSCR records, including signaling/quiet NaNs, both zeros, denormals,
infinities and FZ/DN controls. Both interpreter execution paths pass with
native arithmetic on/off. Semantics follow the [Armv7-M manual](https://documentation-service.arm.com/static/606dc36485368c4c2b1bf62f),
A2.5 FPMinNum/FPMaxNum and A7.7.237. Unsupported coprocessor transfers now
raise NOCP or undefined-instruction faults instead of succeeding silently.
The vendored CPU is revision 0.1.1. Generated tables check their revision and
feature profile, and use deterministic ordering. Normal Rust builds emit an
`rlib`; the reference runner requests its C ABI library explicitly.

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
Set `VIRTUAL_DAISY_EXPECTED_AUDIO_HZ` to an independently established firmware
rate to require agreement within one ppm. Captures retain PLL3 register values
alongside the derived rate; a broad 47–49 kHz check alone misses this regression.

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
