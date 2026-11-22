# Browser verification firmware

`synthphone.bin` is the unchanged raw ARM flash image built from
[Synthphone main at 05d9e4b](https://github.com/UnitedCode/synth-phone-e-v2-rust/commit/05d9e4ba1d7efc8da9f1c246c36898205097e4e1).

- Target: `thumbv7em-none-eabihf`, release profile.
- DSP revision: `4cf708acb2c1744707a316c9fc37bf15b46d8f3e`.
- Size: 130,776 bytes.
- SHA-256: `9cecd47b6940ad1efc4247055546b8ad870b35090a4df40ecfdaf4dcf59ac52b`.

This revision moves RTIC resources to AXI SRAM, leaving approximately 126 KiB
of DTCM for initialization and nested audio interrupt stacks. The previous
56 KiB layout let the emulated stack overwrite the application state during
longer interactive runs. This is a firmware linker fix, not an emulator patch.

The browser starts at its ARM reset vector and executes its startup, interrupt
handlers, UI, MIDI parser, synthesizer and vocal DSP. No audio callbacks or
firmware data are patched. The optional AOT instruction blocks require this
exact image hash; a different image uses the interpreter.

Regenerate in the firmware checkout with `cargo build --release`, then use
`arm-none-eabi-objcopy -O binary` on its resulting ARM ELF. Keep provenance and
the image hash together when updating the fixture.
