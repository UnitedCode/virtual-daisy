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

The local CPU revision is 0.1.18. Generated decoder tables have a checked revision
and deterministic ordering so stale build output cannot quietly omit new masks.
VSEL is decoded only by FPv5 profiles. Decoder tests assert undefined instructions
when a profile lacks double precision or FPv5 rounding/selection instructions.
CI runs the full CPU suite for FPv4-SP, FPv5-SP, FPv5-D16 and no-FPU builds.
It also checks base Armv7-M separately, so DSP-only encodings remain undefined
when the extension is absent.

VCVTB/VCVTT half-storage conversions use an integer kernel for IEEE and ARM
alternative-half formats, direct binary64 narrowing, FPSCR controls and flags.
A 331,776-record QEMU oracle checks both interpreter paths and native settings;
an ARM guest checks every legal register pairing and preserved packed halves.
The public raw-bit conversion API lets the optional Renode adapter use the same
kernel, correcting its unsupported double-half and incorrect inexact flag paths.
That adapter comparison checks integration; QEMU supplies independent math.

USAT, SSAT16 and USAT16 now execute, alongside the existing SSAT. A 520,192-record
QEMU oracle checks every saturation width and legal scalar shift, both lanes,
sticky Q and preserved NZCV/GE. A separate ARM guest checks 15,680 register pairs,
including LR and aliased operands. An integration failure also exposed the
MSR decoder's missing field-mask forms and LDR-literal-to-PC advancing four bytes
past its branch target. EPSR MRS reads no longer leak GE bits.

A further 768 independent QEMU records cover PSR masks/aliases, unprivileged
APSR writes and read-as-zero EPSR fields. Both execution paths pass. The optional
reference adapter uses the same Rust field kernel and repairs its raw GE state
storage for exception frames. Its status-register comparison checks integration;
QEMU supplies independent expectations. A standalone ARM guest also checks GE
preservation through SVC while restoring thread privilege.

QSUB, QDADD and QDSUB now execute alongside QADD. An 8,192-record QEMU oracle
checks the doubled operand's intermediate clamp, sticky Q and unchanged NZCV/GE.
Both execution paths cover every legal register triple, including aliases and LR.
The saturation ARM guest includes all four word operations. Invalid SP/PC operands
fault, and profiles without DSP reject these encodings.

All 36 packed DSP add/subtract forms share a lane kernel, including signed and
unsigned normal, saturating, halving and halfword exchange operations. UADD8's
typed decoder representation is preserved. A 73,728-record independent QEMU
oracle checks results and GE/unchanged flags; both execution paths check every
legal register triple and aliases. Packed saturating operations preserve Q.
The standalone ARM guest includes all forms, with raw-register and APSR checks.

The scalar DSP multiply/absolute-difference families have 229,376 independent
QEMU records covering 28 forms. Eighteen missing forms now execute, including
dual and word-halfword multiplies, accumulates, rounded high words and USAD8.
All legal register/accumulator aliases are checked through both execution paths.
SMULL now sign-extends negative operands before widening; another 2,048 QEMU
records and every legal output/source assignment check the full 64-bit product.
The architectural ARM guest includes a pinned negative SMULL result.

UMAAL, SMLALxy, SMLALD/X and SMLSLD/X now execute. A 292,864-record QEMU oracle
covers these nine DSP-only forms and the four base long multiply instructions,
including unsigned separate-word accumulation, signed halfword/dual products,
64-bit carry/wrap and preserved flags. Both paths check every legal register
assignment and aliases; a standalone ARM guest checks 33,124 register cases.
SP/PC operands and equal output registers fault consistently across all families.

Fixed-point VCVT now has its own decoder rather than falling through to integer
conversion and reading an immediate field as a register number. Every legal
16/32-bit scaling count is checked against 401,408 QEMU records. Signed results
extend to the full destination width; 16-bit results saturate at their own bounds.
The fixed-to-float path uses nearest-even APFloat conversion and exact power-of-two
scaling, avoiding the old generic helper's inaccurate division at 19 fraction bits.

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


Rotated SXTAB/SXTAH/SXTAB16/SXTB16 and UXTAB16 now execute, including all four
rotations and the Rn=PC non-accumulating aliases. UXTAB and UXTB16 require the
DSP extension in both decoding and direct execution. Base SXTB/SXTH/UXTB/UXTH
remain available on Armv7-M. Extension and halfword-packing instructions reject
SP/PC data registers before writing; all preserve APSR. A 229,376-record QEMU
oracle checks all 112 extension/packing encodings, every legal general-register
assignment and aliases through both paths, and skipped IT instructions. The
standalone ARM saturation/packing matrix now has 37,632 cases.


WFE/SEV use a single event latch with reset, exception-return and asynchronous
exception events. SEVONPEND tracks new NVIC pending transitions even while IRQs
are disabled, without inventing events when an existing pending IRQ is enabled.
WFE and WFI have distinct wake conditions under PRIMASK; events never wake WFI.
Sleep/event state participates in transactional rollback. Tests cover both
instruction widths, ordinary/compiled execution, skipped IT conditions, reset,
masked/disabled IRQs and preservation of an event received during WFI. A separate
host-driven ARM guest checks nine sleep checkpoints against Renode and requires
its old no-op event mode to fail. No physical sleep latency is claimed.


LDRT/LDRBT/LDRHT/LDRSBT/LDRSHT and STRT/STRBT/STRHT now share an explicit
unprivileged transfer representation. Loads previously decoded as undefined;
stores used the caller's privileged permissions. Access validation now receives
the forced unprivileged context without changing CONTROL or processor mode.
Fault delivery and transaction replay restore that context. A 262,144-record
QEMU oracle covers all offsets, signed loads, memory guards, flags and aliases;
both execution paths check all legal GPR pairs, including SP bases and LR.
PPB access and CCR.UNALIGN_TRP are enforced before side effects. STIR honors
USERSETMPEND and keeps disabled IRQs pending until the NVIC enables them.


SDIV now returns the architectural wrapped result for MIN/-1 instead of
panicking the host. Division rejects SP/PC operands before access or writes.
A 4,096-record independent QEMU oracle checks signed/unsigned quotients and
unchanged flags through both paths, including every legal GPR triple and
aliases. Separate regressions cover trapped zero division and skipped IT
instructions. The architectural guest appends four pinned division results,
expanding its capture from 100 to 104 words without changing earlier result slots.
