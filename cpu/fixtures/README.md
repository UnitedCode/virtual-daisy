# Independent ARM floating-point fixture

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

Regenerate from the repository root:

```sh
cargo run --release --manifest-path cpu/Cargo.toml --example qemu_isa -- /tmp/isa_oracle.bin
```

Compare the generated file with this fixture before replacing it. The guest is
`isa_oracle.rs`; `isa_oracle.ld` provides its memory map. QEMU's semihosting output
is a local verification mechanism and is not part of the browser runtime.
