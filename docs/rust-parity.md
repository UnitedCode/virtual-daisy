# Rust host parity

The Rust host runs unchanged ARM firmware against the same board, clock, memory,
cache and bus models used by the Renode host. The CPU is the vendored zmu
interpreter, not a Rust port of Renode.

**Functional comparisons pass for the cases below. Full physical-device or
cycle-for-cycle equivalence has not been established.**

## Compared on 22 November 2026

Consumer firmware attachment exposed two additional missing paths. Keyboard
Keyboard's unchanged main ELF reaches `UQADD16`/`UQSUB16` in its drift clamp;
Synthphone's unchanged MIDI receiver uses byte reads of USART1 RDR. The Rust
host now executes the two saturating halfword operations and accepts byte and
halfword RDR/TDR accesses at their exact data-register addresses. Narrow control
register accesses remain outside the implemented contract.

`cpu/tests/device_dsp.rs` runs literal Thumb encodings, checks independent lane
results and unchanged NZCVQ/GE flags. `tests/rust_machine.rs` runs actual ARM
LDRB/LDRH/STRB/STRH instructions, checks receive consumption and verifies two
transmitted bytes remain 320 us apart at 31,250 baud. These tests establish the
emulator paths; complete device acceptance belongs to the project circuit labs.
Sources: [Arm instruction guide, UQADD16/UQSUB16](https://documentation-service.arm.com/static/5e7b6a6216d2907d594035c4),
[ST RM0433 USART registers](https://www.st.com/resource/en/reference_manual/rm0433-stm32h742-stm32h743-753-and-stm32h750-value-line-advanced-armbased-32bit-mcus-stmicroelectronics.pdf).

Reference: Renode 1.17.0 macOS arm64 runtime, with the repository's pinned native
adapter. Infrastructure commit `066a7f13c052215632d469c995c89aea37c573b1`;
tlib commit `167decf9129758829762e582939128c0694e90d6`. Downloads are checksum-checked.
Guests were built with Rust 1.97.1 for `thumbv7em-none-eabihf`, Cortex-M7,
`opt-level=s`. Tests use the same guest sources, linker layout and stimuli.

| Executed firmware | Comparison |
| --- | --- |
| CPU faults and FPU | All 100 result words match for DTCM/shared SRAM stacks and lazy/eager FP saves. Includes divide-by-zero, normal/forced alignment, MPU denied/XN access, NOCP, f64 arithmetic, FP identifiers, scalar/paired VMOV bit transfers and precise fault PCs. |
| Half-precision storage conversion | VCVTB/VCVTT convert directly between FP16 and binary32/binary64, preserving the other packed half. An ARM guest checks 6,144 register pairs, aliases and FPSCR. Restoring the reference's old double-half rejection must fault at the expected instruction. Numerical independence comes from the separate QEMU oracle below. |
| SysTick source selection | An ARM guest compares both CLKSOURCE settings against DWT cycles. The reference suite also restores the old core-only behavior as a required negative control. |
| Progress inside a translated block | A literal 128-NOP ITCM sequence checks progress before the block completes. A second sequence bounds short DWT intervals. Completed-block-only reporting and eight-cycle grouping at 64 MHz are both retained as failing controls. Fractional nanosecond phase is kept in Rust; neither backend has a calibrated M7 pipeline. |
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

### Half-precision conversion

The pinned Renode core rejects FP16/binary64 conversions and reports an ordinary
inexact binary32-to-half conversion as underflow. The adapter now calls the Rust
conversion kernel for all scalar half conversions. This makes the Renode guest
a check of decoding, register transfers and integration; it is **not an
independent numerical oracle** for these instructions.

QEMU 11.1.3 independently executed 331,776 conversion records. These include
every 16-bit encoding in IEEE and alternative-half formats, both full precisions,
all rounding/FZ/DN settings, NaN payloads, boundaries and sticky exception flags.
Both interpreter paths pass with native arithmetic enabled and disabled.
Binary64 narrows directly, avoiding intermediate binary32 rounding. Separate
checks cover IT conditions, disabled coprocessor access and unpopulated D registers.
The Cortex-M7 forms are specified in [Arm's instruction guide, section 3.11.6](https://documentation-service.arm.com/static/61efd6602dd99944d051417b).

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
The vendored CPU is revision 0.1.8. Generated tables check their revision and
feature profile, and use deterministic ordering. Normal Rust builds emit an
`rlib`; the reference runner requests its C ABI library explicitly.

### Nonfused multiply-accumulates

The existing nonfused VMLA/VMLS/VNMLS/VNMLA implementation passes another
49,152 independent QEMU result/FPSCR records. Both precisions, all rounding and
flush/NaN controls, clear/sticky flags and both execution paths are covered with
native arithmetic on/off. These instructions are present in the production
firmware. The audit found no arithmetic mismatch in this corpus; it does not
explain the remaining waveform difference.

### Precision conversions

Single/double precision VCVT instructions now use integer-backed IEEE conversion
instead of host casts. An independent 32,768-record QEMU oracle checks both
directions, all rounding/FZ/DN combinations, signed NaN payloads, subnormal and
overflow boundaries, ties and sticky flags. Both execution paths pass with native
arithmetic on/off. Separate checks cover every legal register pair and source/
destination overlap. The old conversion lost exception flags and ignored the
configured rounding and flush behavior.

The full CPU suite also runs with FPv4-SP, FPv5-SP and no FPU. Unsupported
double-precision arithmetic and FPv5-only selection/rounding instructions must
decode as undefined in the corresponding profiles; the tests keep positive
expectations where supported. The FPv4 scope follows
[ST PM0214, floating-point instruction table](https://www.st.com/resource/en/programming_manual/DM00046982-.pdf).

### Matched current-firmware audio

A separate one-second Rev5 comparison uses that same Rust 1.100 ELF and the
exact `tone.f32le` input retained by the [successful reference CI job](https://github.com/UnitedCode/synth-phone-e-v2-rust/actions/runs/45105129094/job/137464538550):
224.492958 Hz at amplitude 0.2, timestamped at 48,014 Hz. Both hosts interpolate
that serial input onto the derived SAI clock. Rust uses v0.2.3's default timed
runtime with an explicit display-address sink. The reference job uses v0.2.2;
v0.2.3 changes only its host watchdog, not the shared models.

| Observation | Rust | Renode |
| --- | --- | --- |
| Output frames, including startup | 47,562 | 47,582 |
| First frame above 1e-7 | 518 | 483 |
| Derived sample clock | `3256703125/67584` Hz | `3256703125/67584` Hz |

Both complete without CPU fault flags or SAI FIFO errors. The stereo float32
comparison over 47,562 overlapping frames still has **0.165762603 unshifted RMS
error** and 0.271650553 peak absolute error on normalized samples. Skipping the
first 10,000 frames leaves 0.168667254 RMS error; discarding startup does not make
the waveforms match.

The first 4,096-frame diagnostic window reaches 0.021187815 channel-0 RMS error
with a -256-frame right offset. A later window reaches 0.018752754 at +1,494
frames. These short-window searches do not establish a unique delay for a
periodic tone or explain every difference. They are not parity passes. The clock
regression is fixed; exact DSP output and instruction/interrupt pacing still need
further comparison.

A second unshipped one-cycle diagnostic used this exact newer ELF and input.
It produced 47,565 frames, onset at frame 487, and 0.019424766 unshifted RMS
error (0.054696441 peak). Fault and FIFO flags remained clear. This independently
confirms a material pacing contribution for the current firmware. Production
retains its instruction costs; the diagnostic is not a calibrated M7 model or
a waveform parity pass.

The later fractional-progress reference adapter removes whole-cycle grouping
without fitting the waveform. A 200 ms nonperiodic-input comparison still has
9,012 Rust frames versus 9,037 reference frames, with onset at 1,794 versus 1,746.
Unshifted RMS error is 0.180121244 (peak 0.385588408). The first 4,096-frame
constant-offset diagnostic reaches 0.013076096 channel-0 RMS at -48 frames.
Neither this accounting correction nor an offset search establishes waveform
parity. The exact same firmware/input is retained for the next pacing comparison.

### FPv5 rounding and integer conversions

An independent Cortex-M7 QEMU guest adds 39,680 bit-exact result/FPSCR records
for VRINT A/N/P/M/Z/R/X, VCVT A/N/P/M, and existing VCVT/VCVTR integer
conversions. The tests cover both precisions, signed/unsigned destinations,
ties, integer bounds, very large integral values, tiny binary64 inputs, NaNs,
infinities, all applicable FPSCR rounding modes, FZ/DN and sticky exceptions.
Both execution paths pass with native arithmetic on/off. Separate register
tests cover all source/destination pairs, aliasing and the D16 register limit.

These checks found missing instruction decodes, inverted nearest-even integer
ties, overflow reported as OFC instead of IOC, and large/tiny operand errors
in the older generic integral conversion. The implementation now rounds the
encoded IEEE significand directly. Semantics follow
[ST PM0253 Rev6, sections 3.11.33–35](https://www.st.com/resource/en/programming_manual/DM00237416.pdf).
This establishes the tested instruction behavior; it does not settle audio
pacing or the remaining instruction coverage.

Floating-point register-boundary checks also cover every valid single-register
stack list and actual VPUSH/VPOP memory transfers, including untouched words,
registers, FPSCR and stack restoration. Oversized counts are rejected without
integer wraparound in debug and release builds. Raw D16–D31 encodings are
rejected before register conversion can alias them onto D0–D15. Single-only
profiles retain doubleword load/store/move operations but reject double-precision
arithmetic and immediate constants. The register-bank distinction follows
[PM0253 section 4.7](https://www.st.com/resource/en/programming_manual/DM00237416.pdf)
and [Arm's VMOV immediate encoding](https://support.arm.com/documentation/ddi0406/c/Application-Level-Architecture/Instruction-Details/Alphabetical-list-of-instructions/VMOV--immediate-).

A further 61,440 independently executed QEMU records check binary32/binary64
add, subtract, multiply, divide and square root under all four rounding modes,
FZ/DN combinations, clear/sticky flags and boundary/random operands. Both
execution paths pass with native arithmetic enabled and disabled. These caught
incorrect inexact flags for exact results and extreme-operand stalls in the old
generic numerical fallback.

The fallback now uses the [pure Rust LLVM APFloat port](https://github.com/rust-lang/rustc_apfloat),
with ARM NaN priority, FZ, before-rounding tininess and finite-overflow flags
applied explicitly. Square root uses an integer restoring algorithm and exact
remainder rounding. Guarded native arithmetic remains available. These are
instruction-level checks; they do not establish complete FPU or waveform parity.

Fused arithmetic has another 49,152 executed QEMU result/FPSCR records for
VFMA/VFMS/VFNMS/VFNMA in both precisions. All rounding/FZ/DN modes pass on both
execution paths with native arithmetic on/off and sticky-flag replay. Separate
checks cover register aliases, high registers, instruction size and bank limits.
These found a missing VFNMA decode, tiny binary64 results disappearing, incorrect
exception flags, and the wrong NaN sign in VFMS. Fused operations now use one
APFloat rounding step. Boundary checks include an overflowing product canceled
by its addend and a tiny product perturbing the normal/subnormal boundary.

## Supported scope and remaining limits

| Area | Current scope |
| --- | --- |
| Seed revisions | Rev4/AK4556, Rev5/WM8731 and Rev7/PCM3060. No Seed2 DFM or Seed3 profile. |
| CPU timing | Interpreter cycle counts and coupled functional bus service. No calibrated M7 pipeline, dual issue, speculation, store-buffer overlap or exception-entry latency. Startup frame counts and arbitrary timed interrupt PCs can differ. |
| CPU instruction coverage | Thumb interpreter with the tested DSP/FPv5 cases above, including half-precision storage conversion. Fixed-point VCVT instruction paths remain unimplemented; complete ISA coverage has not been established. |
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
