This Apache-2.0 licensed ARM Cortex-M interpreter is derived from
https://github.com/jjkt/zmu/tree/72feb753fea97babc00cc88f2c75e814efc1bfd2/zmu_cortex_m
by Jarmo Torvinen. The upstream license is preserved in LICENSE.

Local changes support STM32H750 browser and native hosts. They include FPv5-D16
and DSP instructions, timed bus hooks, separate instruction fetch, restartable
accesses, architectural fault delivery, NVIC readbacks, SysTick clock selection
and exception frame handling. Scalar/paired VMOV transfers and signed bit-field
extraction have independent register/result checks. Breakpoints deliver debug
exceptions instead of silently continuing; tests cover monitor vector/priority,
HardFault/lockup, status clearing and explicit semihosting interception. Bus
transactions remain opt-in for existing hosts.

FPv5 min/max-number instructions have a separate executed QEMU oracle for both
precisions, NaN payloads, signed zeros and FZ/DN status. Generic MCR2 decoding no
longer captures data-processing encodings, and unsupported coprocessor transfers
fault instead of silently acting as no-ops.
FPv5 integral rounding and directed integer conversion instructions have 39,680
independently executed QEMU records. They cover both precisions, all rounding
modes, FZ/DN, sticky flags, NaNs, infinities and integer boundaries. Conversion
uses the encoded significand directly so very small binary64 values and large
integral operands retain their architectural results. Integer saturation raises
IOC, and nearest-even ties select the even integer. Register-field tests include
high registers, aliased operands and rejection of unpopulated double registers.

The local CPU revision is 0.1.2. Generated decoder tables have a checked revision
and deterministic ordering so stale build output cannot quietly omit new masks.

Changes are checked against the upstream instruction suite, executed ARM guests,
the pinned Renode reference backend and ST's PM0253 programming manual. See
../docs/rust-parity.md for the compared cases and remaining timing limits.
