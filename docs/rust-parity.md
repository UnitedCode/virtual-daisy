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
| CPU faults and FPU | All 104 result words match for DTCM/shared SRAM stacks and lazy/eager FP saves. Includes divide-by-zero, normal/forced alignment, MPU denied/XN access, NOCP, f64 arithmetic, FP identifiers, scalar/paired VMOV bit transfers and precise fault PCs. |
| Half-precision storage conversion | VCVTB/VCVTT convert directly between FP16 and binary32/binary64, preserving the other packed half. An ARM guest checks 6,144 register pairs, aliases and FPSCR. Restoring the reference's old double-half rejection must fault at the expected instruction. Numerical independence comes from the separate QEMU oracle below. |
| Fixed-point conversion | An ARM guest checks 40 QEMU-pinned boundary cases across all populated registers: 960 executions, including signed extension, unsigned saturation and fractional scaling. The separate 401,408-record oracle covers every legal fractional-bit count. |
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
The vendored CPU is revision 0.1.18. Generated tables check their revision and
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

Fixed-point VCVT had been decoded as integer conversion, treating fraction bits
as a source register. It now has a separate decoder and executes both precisions,
signed/unsigned 16/32-bit fixed values, and every legal scaling immediate.
Floating-to-fixed truncates and saturates at the selected integer width; signed
results extend through the full destination register. Fixed-to-float uses
nearest-even rounding and exact power-of-two scaling. The old generic division
helper introduced error at 19 fraction bits, including an incorrect inexact flag
for an exactly representable result.

QEMU independently executed 401,408 records with boundary/seeded inputs and all
FPSCR rounding/FZ/DN combinations. Both interpreter paths pass with native
arithmetic on/off and sticky flags. Register checks cover every legal encoding;
invalid fraction fields and unpopulated double registers remain undefined.
The legal 0–16/1–32 fraction ranges and extension behavior follow
[Arm's fixed-point conversion guide](https://documentation-service.arm.com/static/5e7b6a6216d2907d594035c4), C4.8,
and the [Cortex-M7 guide](https://support.arm.com/documentation/dui0646/c/The-Cortex-M7-Instruction-Set/Floating-point-instructions/VCVT-between-floating-point-and-fixed-point).

## Saturation and PSR integration

USAT, SSAT16 and USAT16 now execute. The existing SSAT and these additions pass
520,192 independently executed QEMU records: every saturation width, legal
scalar shift, representative lane/boundary/seeded inputs, sticky Q, and unchanged
NZCV/GE. Both execution paths pass. DSP-only halfword forms remain undefined in
base Armv7-M; CI now checks that profile separately. A standalone ARM guest tests
15,680 legal source/destination pairs, including LR and aliases.

QSUB, QDADD and QDSUB now execute alongside QADD. Another 8,192 independent
QEMU records cover all four word operations, signed boundaries, seeded operands
and sticky Q with unchanged NZCV/GE. Doubled forms saturate the second operand
before the final add/subtract; either saturation sets Q, even when the final
result fits. Both execution paths check every legal register triple and aliases.
The standalone saturation guest includes all four operations in its register
matrix. SP/PC operands fault, skipped conditions preserve state, and base
Armv7-M rejects these DSP-only encodings.

All 36 packed add/subtract forms now execute: signed/unsigned normal,
saturating and halving byte/halfword operations, including ASX/SAX exchange.
Another 73,728 independent QEMU records check lane results and APSR fields.
Normal forms replace GE from the full lane result; packed saturation and halving
preserve all flags, including Q. This differs from scalar saturation.
Tests cover every legal register triple, aliases, skipped conditions, invalid
registers and reserved encodings. The standalone ARM guest includes every form.
UADD8 keeps its typed instruction representation and uses the same lane kernel.
See [Arm's packed arithmetic definitions](https://arm-software.github.io/acle/main/acle.html#simd32-intrinsics).

The expanded guest found two reference defects: unsigned halfword subtraction
sign-extended its operands before computing GE, and exchanged halfwords wrote
GE to the opposite lane pair. Native adapter revision 16 corrects the existing
C operand representation and GE packing. The reference arithmetic remains
independent of Rust's lane kernel. A saved capture from revision 15 must fail
the register gate at UASX's APSR check.

### DSP multiply and absolute differences

The scalar multiply audit covers 28 forms with 229,376 independent QEMU
records. Eighteen previously missing forms now execute: dual-halfword multiply,
word-by-halfword multiply, their accumulates, high-word accumulates/subtracts,
and byte absolute differences. Rounded high-word forms wrap their intermediate
64-bit sum; Q changes only for the specified overflowing add/accumulate forms.
Both interpreter paths check every legal register and accumulator alias. The
ARM register matrix includes all 28 forms, raising its total to 15,680 cases.

The audit also found that SMULL widened negative operands as unsigned values.
It now sign-extends before multiplication. Another 2,048 QEMU records check
negative/boundary/seeded inputs and preserved flags; register tests cover all
legal output/source assignments and aliases. The architectural ARM guest checks
the pinned negative product before its fault probes. Invalid SP/PC operands and
equal output registers fault. Instruction semantics follow
[Arm's multiply guide](https://documentation-service.arm.com/static/61efd8352dd99944d05142d1), section 3.6.

### Long multiply-accumulates

UMAAL and the eight signed halfword/dual long-accumulate forms now execute.
Another 292,864 QEMU records cover all 13 long multiply forms, including the four
existing base instructions, boundary/seeded operands, accumulator carries/wrap
and preserved APSR fields. Both execution paths check every legal output/source
assignment and aliases. Base Armv7-M rejects the nine DSP-only forms.

A separate ARM guest checks 33,124 register cases across both hosts. It includes
all distinct output-register pairs, every legal second source including LR, and
overlap with its fixed first source. Its instruction trampoline sits separately
from the guest's executable code. Invalid SP/PC and equal output registers fault
consistently across the long instruction families.

That guest exposed three additional Rust bugs. A literal load into PC advanced
another four bytes after branching, and MSR's field masks for GE or combined
APSR fields fell through to branch decoding. EPSR MRS reads also leaked GE bits.
These paths are corrected. Another 768 QEMU records check PSR field masks, aliases,
privileged/unprivileged writes, and read-as-zero EPSR fields. A separate ARM guest
checks 48 pinned cases and requires GE preservation through SVC and return.

The pinned reference omitted GE from its xPSR state transfers and MRS results,
ignored MSR field masks, and required privilege for APSR writes. Native adapter
revision 15 repairs GE storage and uses the Rust PSR field kernel. This checks
integration; QEMU provides independent status-register expectations. Restoring
the old reference PSR behavior must fail the saturation guest's flag check.
The rules follow [Arm's saturation instructions](https://documentation-service.arm.com/static/61efd8352dd99944d05142d1),
sections 3.7.1–2, and [ST PM0253](https://www.st.com/resource/en/programming_manual/DM00237416.pdf),
the core-register summary and MRS/MSR descriptions.

## Startup clock observations

### Uncached instruction service (v0.2.24)

The Rust host requested the same modeled instruction service twice for the two
halves of a Thumb-32 word. It now holds both halves from one service when they
fit within the same line. A word crossing the line boundary still needs its
second transaction. Checks cover transaction counts, holding a fetched word
through a physical-code change, and seeing new uncached bytes on the next
instruction.

The reference previously charged uncached service per translated block/line.
It now uses one-instruction blocks for uncached executable memory. Cached code
keeps ordinary translated blocks; QSPI retains its precise single-instruction
policy. A policy query must not issue bus traffic. Explicit ELF program headers
also keep the probe's ITCM and AXI code in separate, bounded load segments.

The same ARM probe executes 128 narrow and 128 wide NOPs from uncached AXI SRAM.
Baseline-subtracted DWT results are:

| Host | Thumb-16 cycles | Thumb-32 cycles |
| --- | ---: | ---: |
| Rust | 768 | 768 |
| Reference | 631 | 630 |
| Saved old reference policy | 184 | 224 |

The width gate rejects the old capture. Absolute counts still differ: Rust
advances stalled CPUs in whole cycles, while the reference uses fractional MIPS
progress. This checks the declared service model, not M7 pipeline latency.

With the same production ELF and nonperiodic input, the first serial-frame gap
is now 103,637 ns, down from 504,872 ns. At 200 ms Rust produces 9,018 frames and
the reference 9,023, compared with 9,012 and 9,037 before these changes. First
frame edges are 12,860,390 ns and 12,756,753 ns respectively.

Waveform parity remains open. Unshifted RMS difference is 0.180187767, compared
with 0.180121244 before the fetch changes. The signal onset still differs by
about 49 frames at a 0.000001 threshold. Closer startup clocks do not establish
matching audio processing or physical-device behavior.

### Earlier measurements

The same unchanged production ELF and nonperiodic recording were rerun with
clock observations enabled on 23 November 2026. The native, reference and private
pacing-control WAVs each remain byte-identical to their earlier captures.

| Host | First running interval, ns | First serial-frame edge, ns | Frames at 200 ms |
| --- | ---: | ---: | ---: |
| Rust, normal instruction costs | 12,963,872 | 12,984,625 | 9,012 |
| Pinned reference | 12,459,000 | 12,479,753 | 9,037 |
| Private one-cycle retirement control | 12,915,474 | 12,936,227 | 9,015 |

The ordinary Rust first-frame edge is 504,872 ns later than the reference. The
private control still starts 456,474 ns later. This separates startup clock
activation from the later signal-processing delay; changing instruction costs
alone does not remove the startup gap. The control is not shipped.

The timestamps describe the rational clock model since its last reset. They are
not a trace of the enable-register write or calibrated silicon startup time.
The reference observes configuration on one-microsecond ticks; native execution
advances at instruction and bus-service boundaries. A native instruction ended
this run at 200,000,003 ns, three ns beyond the requested boundary.

## Supported scope and remaining limits

| Area | Current scope |
| --- | --- |
| Seed revisions | Rev4/AK4556, Rev5/WM8731 and Rev7/PCM3060. No Seed2 DFM or Seed3 profile. |
| CPU timing | Interpreter cycle counts and coupled functional bus service. No calibrated M7 pipeline, dual issue, speculation, store-buffer overlap or exception-entry latency. Startup frame counts and arbitrary timed interrupt PCs can differ. |
| CPU instruction coverage | Thumb interpreter with the tested DSP/FPv5 cases above, including half-storage and fixed-point conversions. Complete ISA coverage has not been established. |
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
cargo test --locked --release --test arm_guests -- --ignored
```

For a fresh cross-backend comparison, run the reference `cpu` command with an
explicit runtime, output directory, firmware ELF and board profile (see
[reference setup](reference-runner.md)), then:

```sh
VIRTUAL_DAISY_REFERENCE=/path/to/reference-results \
  cargo test --locked --release --test arm_guests -- --ignored
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


### Rotated extensions and packing (v0.2.25)

The Rust CPU now implements the missing SXTAB, SXTAH, SXTAB16, SXTB16 and UXTAB16
forms at all rotations. UXTAB and UXTB16 are correctly restricted to DSP profiles;
base sign/zero extensions remain available on Cortex-M3. The independent QEMU
corpus adds 229,376 records across 112 extension/packing encodings, including all
PKHBT left shifts and PKHTB arithmetic right shifts (encoded zero means 32).
Both execution paths check every legal register assignment, aliases, unchanged
registers/APSR and skipped IT instructions. Invalid SP/PC operands fault before
writes. The same ARM guest now checks 37,632 register pairs against QEMU-pinned
results in both backends. Its longer run budget accommodates additional cases;
it does not calibrate CPU timing. These instructions preserve Q and GE.

Instruction semantics follow the [Cortex-M7 instruction guide](https://documentation-service.arm.com/static/61efd8352dd99944d05142d1),
section 3.8. This closes the audited extension gaps; it does not establish full
instruction, peripheral, audio-waveform or hardware timing parity.


### Event waits (v0.2.26)

WFE and SEV previously acted as no-ops. The Rust CPU now keeps the single event
latch: SEV coalesces events, WFE consumes a latched event or sleeps, and external
events wake WFE without waking WFI. Reset clears the latch and sleep state.
Exception return and preemptible asynchronous exceptions set the latch.
SEVONPEND signals new pending transitions, including disabled interrupts;
reasserting an already pending bit or enabling SEVONPEND after the transition
does not create another event. A PRIMASK-masked enabled interrupt wakes WFI;
without SEVONPEND it does not wake WFE. Both widths and execution paths are
checked, including skipped conditional instructions and preserved APSR.

A host-driven ARM guest stops at nine checkpoints. It distinguishes disabled
pending edges, repeated pending writes, SCR changes, explicit events, masked
interrupts, event retention through WFI and the exception-return event. Both
backends execute the same guest and stimuli. The reference's WFE/SEV-as-no-ops
option is a required failing control. This brings the executed ARM suite to
15 guests. CPU 0.1.16 adds event state to transactional rollback; decoder 15 and
native adapter 16 are unchanged.

The contract follows [Arm's event-register explanation](https://support.arm.com/documentation/ka001269/latest/)
and the [Cortex-M7 guide, power management and WFE/WFI](https://documentation-service.arm.com/static/61efd8352dd99944d05142d1).
This models digital sleep/wakeup behavior. WIC power states, electrical event
inputs, debug halt and physical wake latency remain outside the verified model.

The new event guest also exposed a reference defect. Its pending callback passed
`IsInterruptTargetNonSecure` directly to `SecurityBanked.Get(secure)`. On M7 this
selected the unused SCR bank, suppressing disabled-interrupt events. The managed
adapter now inverts that boolean and verifies the exact pinned patch site at
startup. `tests/fixtures/events_legacy_reference.bin` retains the failed capture.
The Rust event implementation is independent of this reference correction.


### Unprivileged transfers (v0.2.27)

Five unprivileged load forms previously faulted as undefined instructions. Their
three store counterparts incorrectly used the caller's privileged MPU access.
All eight now execute with unprivileged data permissions while retaining the
caller's actual CONTROL, exception mode and fault-delivery context. Rt=Rn loads,
SP bases, LR data registers, every imm8 offset and unchanged APSR are checked.
A 262,144-record QEMU oracle independently pins results and eight-byte memory
guards. All nineteen earlier numerical datasets regenerate unchanged.

A new ARM guest checks 74 cases: seven MPU permission settings, aligned and
unaligned transfers, CCR.UNALIGN_TRP, and USERSETMPEND-controlled STIR writes.
It checks results, untouched bytes, APSR, exact CFSR/MMFAR and the original base
register. The default MPU background does not grant unprivileged access. The
STIR path now retains disabled pending IRQs instead of making them eligible.
The executed ARM suite now has sixteen guests. CPU 0.1.17 uses decoder revision 16.

The reference passed the numerical/MPU cases but incorrectly allowed a user
STIR write with the MPU disabled and USERSETMPEND clear. Native adapter 17 checks
fixed PPB permissions using the access's user MMU index even in privileged
software's STRT/LDRT. It uses a one-use translation for the STIR exception so
permission changes cannot leave a writable user page. Saved native/reference
captures retain the old failures. The Rust and native implementations remain
independent. See the [Cortex-M7 guide](https://documentation-service.arm.com/static/61efd8352dd99944d05142d1),
sections 3.4.4, 4.2.8 and CCR.USERSETMPEND/UNALIGN_TRP. This validates functional
protection behavior; physical bus protection and latency remain unverified.


### Integer division overflow (v0.2.28)

SDIV of signed MIN by -1 previously panicked the Rust host even in a release
build. It now returns 0x80000000, matching independently executed ARM results.
SDIV/UDIV also reject SP/PC operands before side effects. A 4,096-record QEMU
oracle covers boundary/seeded pairs, zero divisors with trapping disabled,
signed truncation and preserved flags. Both paths check every legal register
triple and aliases; separate tests verify CCR.DIV_0_TRP and skipped conditions.

The architectural ARM guest appends MIN/-1, -7/3, unsigned MAX/3 and an untrapped
zero divisor to its existing results. It now captures 104 words; earlier 100 slots
and exception counts retain their meaning. Both backends run the same guest
under DTCM/shared-SRAM and lazy/eager-FP configurations. CPU 0.1.18 keeps decoder revision 16
and native adapter 17; retirement costs remain unchanged and uncalibrated.
All twenty earlier numerical datasets regenerate unchanged. Semantics follow
the [Cortex-M7 instruction guide, SDIV/UDIV](https://documentation-service.arm.com/static/61efd8352dd99944d05142d1)
and the independent QEMU corpus. No hardware division latency is claimed.

### Fractional wait deadlines (v0.2.29)

The Rust host retained fractional nanoseconds when advancing the CPU but
discarded them when converting a peripheral deadline back to CPU cycles. Some
waits therefore advanced one cycle too far. The inverse calculation now uses
the retained phase. RCC writes that leave the core rate unchanged also retain
that phase, including peripheral gate and audio-clock writes.

Two tests failed before the fix. One independently advances the chosen number
of cycles and requires the deadline to be reached on the first eligible edge,
across four integer/fractional rates, 32 starting phases and 256 short waits.
The other checks elapsed time across an unrelated GPIO clock-gate write.
Instruction retirement costs, CPU 0.1.18, decoder 16 and native adapter 17 are
unchanged.

For the fixed nonperiodic production input at 200 ms, the first Rust SAI frame
moves from 12,860,390 ns to 12,824,084 ns. The reference remains at 12,756,753 ns:
the gap falls from 103,637 ns to 67,331 ns. Rust produces 9,020 frames versus
9,023 in the reference, at the same exact rational sample rate and with no CPU
faults or FIFO errors. This does not establish waveform agreement. The 4,000-frame
comparison still finds a 48-frame offset and an aligned RMS difference of
0.012813561881. Physical pipeline and converter timing remain unverified.

### Cortex-M7 control register (v0.2.30)

The Daisy host now gives CCR its M7 reset value, 0x40200. BP and STKALIGN stay
set after writes; reserved fields ignore writes. Other CPU profiles retain
their existing configurable stack alignment. The effective CCR value reaches
the memory model, so reserved input bits cannot change its cache behavior.

Two tests failed before the fix: reset/readback and an executed SVC after
software wrote zero to CCR. The latter starts with SP offset by four bytes and
checks an eight-byte-aligned exception frame, stacked xPSR bit 9, the restored
SP and a clear fault status. Reset and all three board profiles are also checked.
The architectural ARM guest appends four readbacks and the same alignment
exercise, bringing its capture to 112 words. Its extra basic frame adds eight
shared-stack writes: lazy/eager totals are now 144/360.

The old reference returned zero on reset and also allowed STKALIGN to be
cleared. Native adapter 18 now applies the M7 contract independently in reset
and CCR access; its existing exception stacker performs the alignment. No
Rust exception implementation is shared with it. CPU 0.1.19 keeps decoder 16
and unchanged retirement costs. The contract follows the
[Cortex-M7 guide, sections 2.3.7 and 4.3.7](https://documentation-service.arm.com/static/61efd8352dd99944d05142d1).
This checks register and exception behavior, not physical branch-predictor
timing.

### Reserved FPSCR bits on exception return (v0.2.31)

VMSR already masked reserved FPSCR bits, but extended exception return copied
the saved word without that mask. A handler could therefore introduce reserved
bits through the frame. The generic FP exception helper also treated six of
those bits as exception enables and could panic the host.

Exception return now uses the same writable mask as VMSR. Cortex-M floating-point
exceptions accumulate status without enabling that host trap. The helper test
failed with a panic before the fix. The architectural guest independently failed
when its handler set every reserved FPSCR bit in the saved frame; restored
FPSCR must retain the valid rounding control and discard those bits. It checks
both stack regions and lazy/eager preservation without changing the 112-word
capture or frame counters. The classic native reference also preserved those
reserved bits. Adapter 19 masks them in its own FPSCR writer before updating
its floating-point state. The old failing capture is retained. No Rust FP
exception implementation is shared with that correction. Decoder 16 is
unchanged; CPU version is 0.1.20.

The register fields follow the
[Cortex-M7 guide, section 4.7.4](https://documentation-service.arm.com/static/61efd8352dd99944d05142d1).
This fixes exception-state handling; floating-point execution latency remains
uncalibrated.
