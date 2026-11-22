// AudioWorkletProcessor is a JavaScript-only browser interface. This bridge only
// moves PCM; the original ARM firmware in the Rust worker performs every effect.
class FirmwareAudio extends AudioWorkletProcessor {
  constructor() {
    super();
    this.queue = [];
    this.offset = 0;
    this.queued = 0;
    this.capture = new Float32Array(256);
    this.at = 0;
    this.underruns = 0;
    this.playing = false;
    this.enabled = false;
    this.port.onmessage = ({ data }) => {
      if (data.audio) {
        if (this.queued + data.audio.length > sampleRate * 2) this.reset();
        this.queue.push(data.audio);
        this.queued += data.audio.length;
      }
      if (data.reset) this.reset();
      if (data.start) { this.enabled = true; this.at = 0; }
    };
  }
  reset() {
    this.queue = [];
    this.queued = 0;
    this.offset = 0;
    this.playing = false;
  }
  process(inputs, outputs) {
    const out = outputs[0][0];
    const input = inputs[0]?.[0];
    if (!this.playing && this.queued >= sampleRate * 0.012) this.playing = true;
    for (let i = 0; i < out.length; i++) {
      if (this.playing && this.queue.length) {
        const block = this.queue[0];
        out[i] = block[this.offset++];
        this.queued--;
        if (this.offset === block.length) {
          this.queue.shift();
          this.offset = 0;
        }
      } else {
        out[i] = 0;
        if (this.playing) {
          this.playing = false;
          this.underruns++;
        }
      }
      if (this.enabled) this.capture[this.at++] = input ? input[i] : 0;
      if (this.enabled && this.at === this.capture.length) {
        this.port.postMessage({ mic: this.capture, underruns: this.underruns }, [this.capture.buffer]);
        this.capture = new Float32Array(256);
        this.at = 0;
      }
    }
    // The original instrument emits the same signal to both output channels.
    for (let channel = 1; channel < outputs[0].length; channel++) outputs[0][channel].set(out);
    return true;
  }
}
registerProcessor('firmware-audio', FirmwareAudio);
