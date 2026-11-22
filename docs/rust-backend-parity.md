# Rust backend parity audit

**Parity with the Renode backend is incomplete. Physical hardware equivalence
is unverified.** Passing the model or browser tests does not change this status.

The native CLI still uses Renode, its C ARM engine and the C# adapter in `bridge/`.
The browser engine uses a Rust Cortex-M interpreter derived from
[zmu](https://github.com/jjkt/zmu), with local changes. It reuses many Rust board
models. It is a second execution backend; the original has not been replaced
throughout the repository.

This audit compares the browser backend through `bc2d6ae` with the Renode adapter
retained in that tree. The Synthphone checkout inspected on 2026-11-22 pins
Virtual Daisy `749c267`, before the browser backend.

## Coverage and gaps

| Area | Finding / remaining acceptance work |
| --- | --- |
| CPU instructions | Upstream/local instruction tests and independent QEMU float fixtures exist. Extend independent instruction/state comparisons; passing these is not complete ISA coverage. |
| Translated instructions | Native tests shadow against the same Rust interpreter. This checks optimization consistency, independently of Renode/silicon parity. |
| Interrupts | This audit fixes missing SAI/QSPI delivery, disabled-NVIC pending capture and active-handler rising edges. Four regressions execute ARM handlers, check source masking and inspect the NVIC pending latch. Other missing controller interrupts remain part of the timing work below. |
| Data cache/MPU/fabric | Data-cache attributes and QSPI storage are reused, but ordinary accesses and DMA bypass the old timed fabric admission. Port wait/retry, writeback, barriers and exception/vector probes. |
| Instruction fetch | Internal flash is a separate interpreter image. RAM fetch uses data reads instead of `SystemMemory::read_instruction`. Share controller storage and preserve distinct I/D views and runtime fetch checks. |
| SDRAM/internal flash | The browser does not call `SystemMemory::advance_time`. Connect controller completion deadlines, retention/power-state service and their interrupts. |
| DWT | The interpreter counter differs from the old RCC-backed clock adapter. Verify DEMCR trace enable, software lock, readable control and clock changes through CPU MMIO. |
| NVIC readback | The vendored interpreter's ICER/ICPR reads return complements of enable/pending bits; PM0253 specifies the same positive bit meanings as ISER/ISPR. Existing generic unit tests expect the complements. Independent register tests and a core correction are still needed. |
| Boards/codecs | Constructor fixes Rev5 straps and WM8731; it lacks Rev4/Rev7 selection and the old live codec-clock validation. Port profiles, reset wiring and clock checks. |
| Audio | `take_audio` returns the left output sample only. Compare both serial channels with identical firmware, input and event schedules on Renode. |
| ADC/UART/I2C/timer | Browser adapters support the Synthphone path. ADC conversions are rejected and UART TX is immediate. Port carrier scenarios and register/error/deadline checks. |
| Hardware | No independent board capture has been compared; `probe-rs list` found no probe during this audit. Match board revision, codec, firmware, settings and calibrated input before measuring events, latency and analog response. |

The [hardware conformance ledger](conformance.md) describes models available to
the Renode adapter. Its ARM-runtime results do **not** automatically validate
the browser adapter just because both depend on `virtual-daisy`.

## Acceptance evidence

Run the same guest firmware, initial state and timed stimuli through both
backends. Retain commit/firmware hashes, register results, exception order,
stereo PCM and declared tolerances. Negative controls must disable the connection
under test. Booting or generating finite audio is insufficient. Resolve
differences against hardware documentation rather than copying emulator defects.

Run all three suites; root tests exclude the separate CPU and engine workspaces:

```sh
cargo test --release --locked
cargo test --release --locked --manifest-path web/cpu/Cargo.toml \
  --no-default-features --features armv7em,fpv5-d16
cargo test --release --locked --manifest-path web/engine/Cargo.toml
```

Reuse one compatible `CARGO_TARGET_DIR`. Native tests check Rust execution and
adapter behavior. A WASM build additionally checks the browser target; physical
captures remain a separate acceptance gate.

### Results recorded on 2026-11-22

- Shared models/CLI: 226 tests passed after the accessor changes; release
  all-target Clippy with `-D warnings` passed.
- CPU: 481 unit tests and 12 integration tests passed. CPU source is unchanged
  by this audit; existing incorrect NVIC clear-register expectations above
  remain a gap despite the passing suite.
- Engine: 16 unit tests and one integration test passed, including four new
  adapter regressions and all six firmware profiles, audio translation/service
  comparisons, controls and flash persistence.
- The first three adapter regressions failed on the original implementation:
  IRQ87/IRQ92 never reached NVIC, and disabled-NVIC pending capture was lost.
- Release `wasm32-unknown-unknown` engine checking and formatting passed.

The final native engine run used `CARGO_PROFILE_RELEASE_LTO=false` with release
optimization and the normal generated ARM blocks enabled. This avoids a shared
cache's incompatible bitcode artifacts after standalone cdylib/rlib checks;
an attempted thin-LTO rerun reported a missing bitcode section. No production
profile changed; WASM checking retained its normal thin-LTO profile. Reproduce
this native run by prefixing the engine test command above with that environment
variable. These checks establish the stated regression results, not global
Renode parity, calibrated deadlines or a physical-board match.

ST's [PM0253, sections 4.2.2 and 4.2.10](https://www.st.com/resource/en/programming_manual/pm0253-stm32f7-series-and-stm32h7-series-cortexm7-processor-programming-manual-stmicroelectronics.pdf)
specifies pending capture even while NVIC-disabled; enable gates delivery.
Its cache/barrier chapters and [RM0433](https://www.st.com/resource/en/reference_manual/rm0433-stm32h743-753-and-stm32h750-value-line-advanced-arm-based-32-bit-mcus-stmicroelectronics.pdf)
provide hardware expectations for further comparisons.
