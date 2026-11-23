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

`events_legacy_reference.bin` is the 128-byte result from the new event guest
before correcting the pinned NVIC's SEVONPEND bank choice. Native adapter 16 was
unchanged. Its SHA256 is
`8b638d8865706c1b18295fbd78aad8fbe4a78509a1b872e5333d5c114a734098`.
The nine observed stages are `1,1,1,1,1,3,5,7,7`, instead of
`1,3,3,5,5,7,9,9,12`: a newly pending disabled IRQ never supplies its event.
The guest only advances on later explicit events and does not finish. The
saved capture must fail the gate. The adapter now negates the callback's
"target nonsecure" boolean before passing it to `SecurityBanked.Get(secure)`.
The guest, stimuli and checker remain in `guests/events.rs`,
`tests/arm_guests.rs`, `bridge/DaisyHost.cs` and `src/events_probe.rs`.
