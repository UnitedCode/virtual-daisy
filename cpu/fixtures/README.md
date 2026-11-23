# Independent ARM instruction fixtures

`isa_oracle.bin` contains 1,050 records of five little-endian `u32` values:
operand A, operand B, operation (add/subtract/multiply/divide/square root), result, FPSCR.
The fixture executes ARM VFP instructions under QEMU's Cortex-M7 machine,
independently of this interpreter. It includes signed zero, normal/subnormal
values, infinities, NaN payloads and deterministic pseudorandom inputs.
The checked-in fixture was generated with QEMU 11.1.3 and rustc 1.97.1.

`isa_oracle_f64.bin` adds 1,720 binary64 add/subtract/multiply/divide/compare
records. Each has eight little-endian words: A low/high, B low/high, operation,
result low/high, and FPSCR. Comparison records check FPSCR; their result words
are unused. Both decoded and ordinary execution are checked with clear and
preexisting IXC flags. The generator writes this companion as `isa_oracle.f64.bin`.
It includes every pairing of signed zeros, infinities, signaling/quiet NaNs and
finite boundary values, plus 200 deterministic random pairs.

`isa_oracle_control.bin` contains another 9,632 binary32 records covering the
five arithmetic operations and both comparison variants, with the same
special-value matrix and 200 random pairs, each
with flush-to-zero and default-NaN modes independently on/off. Its six words
are A, B, operation, initial FPSCR, result, and final FPSCR. Both instruction
execution paths are checked with clear and sticky IXC. The generator writes
this companion as `isa_oracle.control.bin`.

`isa_oracle_minmax.bin` adds 7,296 FPv5 VMAXNM/VMINNM records, covering both
precisions, all pairings of 16 special values, 200 deterministic random pairs,
and independently enabled FZ/DN controls. Its ten words are precision, A low/high,
B low/high, operation (0=max, 1=min), initial FPSCR, result low/high, final FPSCR.
The initial status includes NZCV and sticky IXC. Tests check ordinary/compiled
execution with native arithmetic enabled and disabled. The generator writes
`isa_oracle.minmax.bin`; QEMU 11.1.3 and rustc 1.97.1 generated the fixture.

`isa_oracle_rounding.bin` contains 39,680 nine-word records from
`rounding_oracle.rs`: magic `0x544e5256`, precision, input low/high, operation,
initial FPSCR, result low/high, final FPSCR. Operations are VRINT A/N/P/M/Z/R/X
(0–6), VCVT A/N/P/M signed/unsigned (7–14), truncating signed/unsigned VCVT
(15–16), and FPSCR-controlled signed/unsigned VCVTR (17–18). Single-word results
have a zero high word. The fixture covers both precisions, all four FPSCR
rounding modes where applicable, FZ/DN combinations and clear/preexisting flags.
Each precision uses 32 boundary/special inputs and 128 deterministic random
inputs. QEMU 11.1.3 and rustc 1.97.1 reproduce this companion; the four earlier
fixtures regenerate byte-for-byte unchanged. The generator writes
`isa_oracle.rounding.bin`. Tests exercise both execution paths with native
arithmetic on/off, check untouched registers and APSR, and separately vary all
legal source/destination pairs, including aliases.

`isa_oracle_arithmetic_rounding.bin` adds 61,440 eleven-word records:
precision, A low/high, B low/high, operation (add/subtract/multiply/divide/sqrt),
initial FPSCR, result low/high, final FPSCR, magic `0x48545241`.
`arithmetic_rounding_oracle.rs` executes each operation under all four rounding
modes and FZ/DN combinations, with clear/preexisting status flags. Each precision
uses all pairs of 16 boundary/special values and 128 deterministic random pairs.
The generator writes `isa_oracle.arithmetic.bin`; QEMU 11.1.3 and rustc 1.97.1
reproduce it and all five earlier datasets unchanged. Both interpreter paths are
checked with native arithmetic enabled and disabled. The binary64 cases include
square root at tiny and very large magnitudes.

`isa_oracle_fused.bin` contains 49,152 thirteen-word records from `fused_oracle.rs`:
precision, A low/high, B low/high, addend low/high, operation (VFMA/VFMS/VFNMS/VFNMA),
initial FPSCR, result low/high, final FPSCR, magic `0x45535546`. Each precision
uses 256 special triples plus 128 seeded/targeted triples, with all rounding and
FZ/DN combinations. Every executed record starts with clear exception flags;
tests also replay it with sticky flags. Cases include product overflow canceled
by the addend and tiny products perturbing the normal/subnormal boundary.
QEMU 11.1.3 and rustc 1.97.1 reproduce the fixture and the six earlier datasets
unchanged. The generator writes `isa_oracle.fused.bin`.

`isa_oracle_precision.bin` adds 32,768 seven-word records from
`precision_oracle.rs`: direction (0=binary32 to binary64, 1=reverse), input
low/high, initial FPSCR, result low/high and final FPSCR. Each direction uses
1,024 boundary/seeded inputs under all rounding and FZ/DN combinations. Cases
include NaN signs/payloads, overflow, exact subnormals, ties and values immediately
around the binary32 normal/subnormal boundary. Tests also replay sticky flags
and cover every legal register pairing, including overlapping source/destination
storage. QEMU 11.1.3 and rustc 1.97.1 reproduce this fixture and all seven earlier
datasets unchanged. The generator writes `isa_oracle.precision.bin`.

`isa_oracle_accumulate.bin` adds 49,152 nonfused VMLA/VMLS/VNMLS/VNMLA records
from `accumulate_oracle.rs`. Its thirteen-word layout matches the fused fixture
with magic `0x414c4d56`; the generator distinguishes their record types by this
trailer. Both precisions, all rounding/FZ/DN settings, exceptional values and
seeded/targeted triples are executed independently by QEMU. Tests cover both
execution paths, native arithmetic on/off and clear/sticky flags. All eight
earlier datasets regenerate byte-for-byte unchanged. The generator writes
`isa_oracle.accumulate.bin`.

`isa_oracle_half.bin` contains 331,776 nine-word records from `half_oracle.rs`:
operation, input low/high, destination canary, initial FPSCR, result low/high,
final FPSCR, magic `0x464c4148`. Operations 0/1 widen bottom/top FP16 to binary32;
2/3 narrow binary32 to bottom/top FP16; 4/5 widen to binary64; 6/7 narrow binary64.
It exhausts all 65,536 half encodings in both IEEE and alternative formats for
both destination precisions, then checks control combinations and 512 narrowing
inputs per full precision. QEMU 11.1.3 and rustc 1.97.1 reproduce this companion
and all nine earlier datasets unchanged. The generator writes
`isa_oracle.half.bin`. Tests replay clear/sticky status through both execution
paths with native arithmetic on/off, and check every legal register pairing.

`half_probe.rs` checks the separate ARM guest's eight numerical boundary cases
and 6,144 register-pair results. `half_legacy_reference.bin` is a 128-byte result
capture plus the four-byte fault instruction from the pinned Renode backend
with its old double-half rejection restored. It must fail the positive gate and
pass the negative gate, with an undefined-instruction fault at `0xeeb20b41`.
The repaired reference uses the Rust numerical kernel; only QEMU is independent
for half-conversion arithmetic.

`isa_oracle_fixed.bin` contains 401,408 eight-word records from `fixed_oracle.rs`:
literal opcode, input low/high, initial FPSCR, result low/high, final FPSCR,
magic `0x44584946`. The 392 legal operation/precision/width/fraction combinations
each use 64 boundary/seeded operands under all rounding/FZ/DN combinations.
Sixteen-bit fixed formats allow 0–16 fraction bits; 32-bit formats allow 1–32.
The guest executes literal instructions from writable RAM with DSB/ISB between
changes, independently of the interpreter's decoder and numerical code.
QEMU 11.1.3 and rustc 1.97.1 reproduce this companion and all ten earlier binary
datasets unchanged. The generator writes `isa_oracle.fixed.bin`.

The same generator extracts 40 boundary records as `isa_oracle.fixed-probe.rs`;
`fixed_probe_cases.rs` keeps those values for the standalone ARM guest. Its 960
executions cover all populated registers for selected endpoint/interior scales.
CPU tests require every selected row to exist in the full independent fixture.

`isa_oracle_saturate.bin` contains 520,192 six-word records from
`saturate_oracle.rs`: opcode, input, initial APSR, result, final APSR, magic
`0x54415353`. SSAT/USAT cover all 32 widths, 63 legal scalar shifts and 32
boundary/seeded values under four NZCV/Q/GE starting states. SSAT16/USAT16 cover
all 16 widths with the same inputs and flags. QEMU 11.1.3 and rustc 1.97.1
reproduce this fixture and all eleven earlier datasets unchanged. The generator
writes `isa_oracle.saturate.bin` and extracts 12 cases as
`isa_oracle.saturate-probe.rs` for `saturation_probe_cases.rs`.

`isa_oracle_psr.bin` adds 768 eight-word records from `psr_oracle.rs`: opcode,
input, initial APSR, unprivileged flag, final APSR, MRS XPSR, MRS EPSR and magic
`0x52535041`. Each APSR-bearing alias, field mask and privilege level uses eight
operands and four initial flag states. Unprivileged cases return through an SVC
handler before semihosting output. All twelve earlier datasets remain unchanged.
The generator writes `isa_oracle.psr.bin` and extracts 48 cases as
`isa_oracle.psr-probe.rs` for the separate ARM guest. That guest also requires
APSR/GE preservation across exception entry and return.

The native saturation guest's dynamic instruction wrapper preserves its caller's
stack and loads/stores all general registers, including LR. This tests every
legal register pair without using a tested operand as a branch-return address.
CPU tests require all extracted guest cases to occur in the independent binary.

`isa_oracle_qadd.bin` contains 8,192 seven-word records from `qadd_oracle.rs`:
operation (QADD/QSUB/QDADD/QDSUB), first input, second input, initial APSR,
result, final APSR and magic `0x44444151`. Each operation uses 256 boundary
pairs and 256 seeded pairs under four NZCV/Q/GE states. QEMU executes literal
assembler instructions, including the doubled operand's intermediate saturation.
The generator writes `isa_oracle.qadd.bin`; `isa_oracle.qadd-probe.rs` extracts
four cases for `qadd_probe_cases.rs`. Those extend the ARM guest to 3,136 register
pairs. CPU checks cover every legal register triple and aliases on both paths.
All thirteen earlier binary datasets regenerate unchanged.

Regenerate from the repository root:

```sh
cargo run --release --manifest-path cpu/Cargo.toml --example qemu_isa -- /tmp/isa_oracle.bin
```

Compare the generated file with this fixture before replacing it. The guest is
`isa_oracle.rs`; `isa_oracle.ld` provides its memory map. QEMU's semihosting output
is a local verification mechanism and is not part of the browser runtime.
