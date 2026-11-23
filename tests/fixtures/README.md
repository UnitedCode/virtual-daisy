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


`unprivileged_legacy_native.bin` is the 2,432-byte ARM guest capture from released
v0.2.26 (merge `0c6c84e37bcd01d8e2f898c0c91757ca9c0d8bd7`). Its SHA256 is
`f9237c568b28f8304a487c35bb442a2cc9cd331c19ddcf23b07d67a0944335e8`.
The first unprivileged byte load raises UNDEFINSTR instead of the expected MPU
fault. The old STIR path later delivers disabled IRQ5 and stops in the unexpected
handler (IPSR21). Both failures are pinned independently of a completion marker.

`unprivileged_legacy_reference.bin` is the 2,432-byte result with native adapter16.
Its SHA256 is
`003791151d1394f39d678c8c842c482751b2c23708241ae17adf839577b5b7a6`.
It completes all 74 cases but case72 incorrectly accepts STRT to STIR with
USERSETMPEND clear: pending IRQ5 is set, CFSR remains zero. The gate expects
DACCVIOL with a valid MMFAR and no pending IRQ. Adapter17 enforces PPB access
before the MPU-disabled fast path; the saved result must still fail.
