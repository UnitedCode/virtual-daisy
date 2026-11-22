# Synthphone in the browser

Power on, enable the microphone, then pick up the receiver. Play the photographed
keys and drag the real side dials. MIDI files and MIDI input/output are in the
small toolbar; keyboard shortcuts are under **?**.

The Rust WebAssembly backend boots the real ARM firmware. Its UI, OLED, synth,
vocal effects and saved settings run on that CPU. The photo layers preserve the
actual instrument; see [asset provenance](phone/assets/README.md).

**Experimental:** live vocal audio can fall behind on slower browsers. The
firmware lab renders complete clips and offers WAV downloads. Processing stays
on the visitor's computer; the host serves static files. Timing is functional,
not hardware calibrated.

<details><summary>Build and run locally</summary>

Install the `wasm32-unknown-unknown` Rust target and `wasm-bindgen-cli` version
`0.2.127`. From this repository:

```sh
CARGO_TARGET_DIR=target cargo run --manifest-path web/site-builder/Cargo.toml -- build
CARGO_TARGET_DIR=target cargo run --manifest-path web/site-builder/Cargo.toml -- serve
```

The preview binds `127.0.0.1:3030`. Check its ownership before launching it;
stop your preview when finished. Deploy `dist` to any static HTTPS
host. No Renode installation, login, hardware, backend, or paid API is required
for visitors.

For agent work, reuse an existing compatible `CARGO_TARGET_DIR`.

To publish merged source to GitHub Pages, sign in with `gh` and run:

```sh
CARGO_TARGET_DIR=target cargo run --manifest-path web/site-builder/Cargo.toml -- pages
```

This builds locally, publishes the runtime bundle to `gh-pages`, and enables
Pages. It requires a clean checkout whose commit is already on `origin/main`.

</details>

<details><summary>Verification and implementation</summary>

```sh
cargo test --release --manifest-path web/engine/Cargo.toml
cargo test --release --manifest-path web/cpu/Cargo.toml --no-default-features --features armv7em,fpv5-d16
cargo test
```

The engine connects a Rust Cortex-M interpreter to Virtual Daisy's RCC, memory,
GPIO, QSPI, DMA, SAI and codec models. Browser-specific Rust adapters provide
TIM2, USART1, the blocking I2C buses, and the SSD1306 display. ADC coverage
currently includes the firmware's initialization sequence, not general ADC
conversions. Oscillator readiness is immediate; analog transients and bus timing
are not calibrated. Unmapped accesses stop with an error.

Short sequences are decoded at build time into Rust instruction calls and
guarded register-local expressions. These still execute CPU instruction
semantics and require the matching firmware hash. In native tests, every
accepted lifted block is shadowed by interpretation and compared bit for bit:
FP and general registers, flags, changed memory, PC, instruction count and cycles.
Differential tests also compare complete audio with interpretation.
Busy idle and decrement loops have bounded CPU-state-preserving coalescing.
Board service follows audio, timer, UART, encoder and flash deadlines. MMIO
accesses synchronize first; interrupt boundaries stop translated execution.
Browser slices also stop after two virtual milliseconds, keeping captured mic
input and queued controls responsive. A finer polling schedule is checked
against the same firmware's PCM output. A transferred message port connects the
firmware worker directly to the audio thread; UI rendering does not relay PCM.
Capture packets are 256 frames and playback starts with 12 ms queued.

Generate repeatable test input and measure the real ARM firmware in V8:

```sh
rustc -O web/engine/benchmark-fixture.rs -o /tmp/synthphone-fixture
/tmp/synthphone-fixture /tmp/synthphone-voice.f32 voice
node web/engine/benchmark.mjs dist /tmp/synthphone-voice.f32 6 3 full
/tmp/synthphone-fixture /tmp/synthphone-stress.mid midi
```

Profiles are 0 Pitch Control, 1 Vocode, 2 Dry, 3 Tuned, 4 Harmony and 5 Percussion.
`full` adds eight MIDI voices, drums and octave/formant controls. The generated
60-second MIDI file exercises the same controls through the phone's file player.

The vendored interpreter's source, Apache license and upstream revision are in
`cpu`. Regression tests cover interrupt IT state, NVIC banks, conditional float
moves, instruction flags, and 840 independently executed QEMU ARM float cases
(also tested with sticky IXC). Firmware tests check actual microphone processing,
silent input, MIDI channel routing, keypad scanning, encoder menus, hook channel
selection, sustained streaming, and firmware QSPI save/restore across power
cycles. Nonvolatile flash is stored in localStorage; microphone samples are not
persisted. These are simulated checks, not physical
hardware or listening acceptance.

MIDI channel 1 controls vocal effects silently. Channel 2 plays the synthesizer;
the lab's audible A4 test uses channel 2. This follows the firmware's routing.

</details>
