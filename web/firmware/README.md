# Browser verification firmware

`synthphone.bin` is the unchanged raw ARM flash image built from
[Synthphone main at 2084ca9](https://github.com/UnitedCode/synth-phone-e-v2-rust/commit/2084ca93a36fb60b6c4fec4947f2376c4ef08dac).

- Target: `thumbv7em-none-eabihf`, release profile.
- DSP revision: `0057fbb416fa1e7abb18364dd9048ff2e78f8ad5`.
- CPU: `cortex-m7`, with the DSP `arm-hardware-float` feature.
- Size: 127,824 bytes.
- SHA-256: `3613b4a6f4a652a8b8b1117a4b5baac54e67e1f650eb8a57d5778c9e4940befb`.

The firmware shares its existing bounded-phase sine table with oscillator voices
and phase-vocoder synthesis, avoiding software-double trigonometry in audio-rate
loops. Its phase progression stays the same; sine/cosine component error against
`libm` is below 0.000005. This optimization is in the ARM firmware itself.
FFT magnitudes use the real FPU's `VSQRT.F32` instruction, preserving binary32
square-root results while avoiding software arithmetic. The browser executes
these instructions through its CPU model.

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
