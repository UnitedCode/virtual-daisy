# Browser verification firmware

`synthphone.bin` is the unchanged raw ARM flash image built from
[Synthphone main at 6b715db](https://github.com/UnitedCode/synth-phone-e-v2-rust/commit/6b715db).

- Target: `thumbv7em-none-eabihf`, release profile.
- DSP revision: `4cf708acb2c1744707a316c9fc37bf15b46d8f3e`.
- Size: 130,776 bytes.
- SHA-256: `04739d5ffaa588b8f50a3cf922be8093dcf3e921636968c7879c240db43b3b74`.

The browser starts at its ARM reset vector and executes its startup, interrupt
handlers, UI, MIDI parser, synthesizer and vocal DSP. No audio callbacks or
firmware data are patched. The optional AOT instruction blocks require this
exact image hash; a different image uses the interpreter.

Regenerate in the firmware checkout with `cargo build --release`, then use
`arm-none-eabi-objcopy -O binary` on its resulting ARM ELF. Keep provenance and
the image hash together when updating the fixture.
