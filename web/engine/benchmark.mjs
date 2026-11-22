// Minimal V8/wasm-bindgen harness. Rust generates the input fixture; every audio
// sample is produced by the original ARM image, with no JavaScript DSP.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { pathToFileURL } from 'node:url';
const [directory, fixture, secondsText = '6', profileText = '0', stimulus = 'vocal', firmwarePath] = process.argv.slice(2);
if (!directory || !fixture) throw new Error('Usage: node benchmark.mjs <module directory> <stereo input.f32> [seconds] [profile 0..5]');
const seconds = Number(secondsText), profile = Number(profileText);
if (!(seconds >= 2 && seconds <= 1200) || !Number.isInteger(profile) || profile < 0 || profile > 5) throw new Error('Invalid benchmark duration or profile');
const { default: init, Firmware } = await import(pathToFileURL(path.resolve(directory, 'engine.js')));
await init({ module_or_path: fs.readFileSync(path.join(directory, 'engine_bg.wasm')) });
const bytes = fs.readFileSync(fixture);
const input = new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / 4);
const f = new Firmware(fs.readFileSync(firmwarePath || new URL('../firmware/synthphone.bin', import.meta.url)));
f.handset(true);
function step() {
  if (f.pending_input() < 4096) f.input_stereo(input);
  f.run_for(100000, 2000000n);
  return f.take_audio();
}
function advance(milliseconds) {
  const until = f.virtual_time_ns() + BigInt(milliseconds) * 1000000n;
  while (f.virtual_time_ns() < until) step();
}
advance(500);
for (let mode = 0; mode < profile; mode++) {
  f.key(10, true); advance(40); f.key(10, false); advance(40);
}
f.midi(new Uint8Array([0x90, 69, 100]));
if (stimulus === 'full') f.midi(new Uint8Array([0x91,48,100,52,90,55,90,59,80,60,80,64,80,67,80,71,80]));
if (stimulus !== 'vocal') f.midi(new Uint8Array([0xb0,80,127,81,127]));
advance(250);
const target = Math.floor(f.sample_rate() * seconds);
const hash = crypto.createHash('sha256');
let samples = 0, peak = 0;
const start = performance.now();
let nextDrum=f.virtual_time_ns();
let drum=0;
while (samples < target) {
  if (stimulus === 'full' && f.virtual_time_ns() >= nextDrum) {
    f.midi(new Uint8Array([0x99,[36,42,38,49][drum++%4],100]));
    nextDrum+=125000000n;
  }
  const output = step();
  const count = Math.min(output.length, target - samples);
  hash.update(new Uint8Array(output.buffer, output.byteOffset, count * 4));
  for (let i = 0; i < count; i++) peak = Math.max(peak, Math.abs(output[i]));
  samples += count;
}
const wallSeconds = (performance.now() - start) / 1000;
console.log(JSON.stringify({ profile, stimulus, audioSeconds: seconds, wallSeconds, speed: seconds / wallSeconds, samples, peak, sha256: hash.digest('hex'), stats: f.stats() }));
f.free();
