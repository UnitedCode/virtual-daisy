# Browser verification firmware

`synthphone.bin` is the unchanged raw ARM flash image built from
[Synthphone main at f473730](https://github.com/UnitedCode/synth-phone-e-v2-rust/commit/f47373010d052bc7791a998e3e596b989159c49d).

- Target: `thumbv7em-none-eabihf`, release profile.
- DSP revision: `23ec6a94548bd0c57b968aef553ab38c4ddbd3b8`.
- Size: 131,032 bytes.
- SHA-256: `d99eb1f0986d026f7204cac1a85e53dc17805250b9b54820944b65498ba83cd0`.

The firmware shares its existing bounded-phase sine table with oscillator voices
and phase-vocoder synthesis, avoiding software-double trigonometry in audio-rate
loops. Its phase progression stays the same; sine/cosine component error against
`libm` is below 0.000005. This optimization is in the ARM firmware itself.

RTIC resources live in AXI SRAM, leaving approximately 126 KiB
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
