# Independent ARM floating-point fixture

`isa_oracle.bin` contains 840 records of five little-endian `u32` values:
operand A, operand B, operation (add/subtract/multiply/divide), result, FPSCR.
The fixture executes ARM VFP instructions under QEMU's Cortex-M7 machine,
independently of this interpreter. It includes signed zero, normal/subnormal
values, infinities, NaN payloads and deterministic pseudorandom inputs.
The checked-in fixture was generated with QEMU 11.1.3 and rustc 1.97.1.

Regenerate from the repository root:

```sh
cargo run --release --manifest-path web/engine/Cargo.toml --example qemu_isa -- /tmp/isa_oracle.bin
```

Compare the generated file with this fixture before replacing it. The guest is
`isa_oracle.rs`; `isa_oracle.ld` provides its memory map. QEMU's semihosting output
is a local verification mechanism and is not part of the browser runtime.
