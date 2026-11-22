# Synthphone in the browser

The Rust browser backend boots the original ARM Synthphone firmware and captures
its audio output. The firmware lab offers microphone-tone, MIDI-note and silence
tests, an actual OLED view, and WAV downloads. Everything runs on the visitor's
computer; hosting only serves static files.

**Status:** browser boot and rendered audio work. Real-time microphone performance
and the finished interactive phone are still in development. This is a
functional firmware emulator, not a cycle-calibrated hardware simulation.

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

Short sequences can be decoded at build time into Rust instruction calls. These
still execute CPU instruction semantics and are enabled only for the matching
firmware hash. Differential tests compare their audio with interpretation.
Busy idle and decrement loops have bounded CPU-state-preserving coalescing.

The vendored interpreter's source, Apache license and upstream revision are in
`cpu`. Regression tests cover interrupt IT state, NVIC banks, conditional float
moves, instruction flags, and 840 independently executed QEMU ARM float cases
(also tested with sticky IXC). Firmware tests check actual microphone processing,
silent input, and MIDI channel routing. These are simulated checks, not physical
hardware or listening acceptance.

MIDI channel 1 controls vocal effects silently. Channel 2 plays the synthesizer;
the lab's audible A4 test uses channel 2. This follows the firmware's routing.

</details>
