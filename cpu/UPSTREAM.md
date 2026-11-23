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

Raw floating-point register bits are checked before conversion to register enums
can discard them. Unpopulated double registers fault; single-register stack lists
use the correct S-register fields and widened list bounds. Tests cover all valid
single stack lists, exact memory transfers, malformed counts in debug/release,
and double-bank boundaries. Single-only profiles keep doubleword storage moves
while rejecting double-precision arithmetic.

The local CPU revision is 0.1.7. Generated decoder tables have a checked revision
and deterministic ordering so stale build output cannot quietly omit new masks.
VSEL is decoded only by FPv5 profiles. Decoder tests assert undefined instructions
when a profile lacks double precision or FPv5 rounding/selection instructions.
CI runs the full CPU suite for FPv4-SP, FPv5-SP, FPv5-D16 and no-FPU builds.

Changes are checked against the upstream instruction suite, executed ARM guests,
the pinned Renode reference backend and ST's PM0253 programming manual. See
../docs/rust-parity.md for the compared cases and remaining timing limits.

Software binary arithmetic uses rustc_apfloat 0.2.3+llvm-462a31f5a5ab
(https://github.com/rust-lang/rustc_apfloat), licensed under Apache-2.0 WITH
LLVM-exception. ARM-specific NaN priority, input/output flush-to-zero, tininess
before rounding and finite-overflow flags are applied around its IEEE arithmetic.
Square root uses a restoring integer algorithm with exact remainder rounding.
Binary32/binary64 precision conversion also uses APFloat, with ARM NaN payload
conversion, FPSCR rounding, flush controls and sticky exception flags. Its
independent 32,768-record oracle covers both instruction execution paths and
every legal source/destination pairing, including overlapping register storage.
Guarded native arithmetic remains available. The independent arithmetic oracle
checks all rounding modes on both execution paths with native arithmetic on/off.

VFMA/VFMS/VFNMS/VFNMA also use single-rounding APFloat arithmetic. VFNMA now
decodes for both precisions; VFMS negates the specified first multiplicand so
NaN payload signs agree with the executed ARM oracle. Its 49,152 clear-status
records cover all rounding/FZ/DN modes, seeded/special triples and cancellation
at the normal/subnormal boundary. Tests replay sticky flags and check high
registers, source/destination aliases, instruction size and D16-bank rejection.
