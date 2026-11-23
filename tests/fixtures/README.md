# Captured negative controls

`uncached_fetch_legacy_reference.bin` is the 128-byte instruction-clock result
from the v0.2.23 reference service policy, using native adapter 16 and the new
AXI SRAM width probe. The frozen Rust C ABI library differs from release source
only by private RCC/CCR trace logging; logging was disabled for this capture.

It executed 128 narrow and 128 wide NOPs with the instruction cache off. The
baseline-subtracted DWT results were 184 and 224 cycles. Its old ITCM and short
interval checks pass; the new equal-service width gate must reject it. The
reference formerly requested service per translated block/line. The corrected
policy uses one-instruction blocks for uncached executable memory.

This is a functional service check, not measured Cortex-M7 pipeline timing.
The probe, linker layout and gate are editable in `guests/instruction_clock.rs`,
`guests/cpu.ld` and `src/instruction_clock_probe.rs`.
