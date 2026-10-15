// This asset runs directly in AudioWorkletGlobalScope. Keep it self-contained.
const SAMPLE_RATE = 16_000;
const MAX_FRAMES = SAMPLE_RATE * 120;
const WAV_HEADER_BYTES = 44;
const MAX_BYTES = WAV_HEADER_BYTES + MAX_FRAMES * 2;

class AnimaPcmCaptureProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.buffer = new ArrayBuffer(MAX_BYTES);
    this.view = new DataView(this.buffer);
    this.frames = 0;
    this.terminal = false;
    this.port.onmessage = (event) => {
      if (this.terminal) return;
      const message = event.data;
      if (!message || message.version !== 1) {
        this.error('invalid_audio');
      } else if (message.type === 'cancel') {
        this.error('empty');
      } else if (message.type === 'stop') {
        this.complete(false);
      } else {
        this.error('invalid_audio');
      }
    };
  }

  error(reason) {
    if (this.terminal) return;
    this.terminal = true;
    this.buffer = null;
    this.view = null;
    this.port.postMessage({ version: 1, type: 'error', reason });
  }

  complete(limitReached) {
    if (this.terminal) return;
    if (this.frames === 0) {
      this.error('empty');
      return;
    }

    const usedBytes = WAV_HEADER_BYTES + this.frames * 2;
    const view = this.view;
    const buffer = this.buffer;
    const writeFourCC = (offset, value) => {
      for (let i = 0; i < 4; i++)
        view.setUint8(offset + i, value.charCodeAt(i));
    };
    writeFourCC(0, 'RIFF');
    view.setUint32(4, usedBytes - 8, true);
    writeFourCC(8, 'WAVE');
    writeFourCC(12, 'fmt ');
    view.setUint32(16, 16, true);
    view.setUint16(20, 1, true);
    view.setUint16(22, 1, true);
    view.setUint32(24, SAMPLE_RATE, true);
    view.setUint32(28, SAMPLE_RATE * 2, true);
    view.setUint16(32, 2, true);
    view.setUint16(34, 16, true);
    writeFourCC(36, 'data');
    view.setUint32(40, this.frames * 2, true);

    this.terminal = true;
    this.buffer = null;
    this.view = null;
    this.port.postMessage(
      {
        version: 1,
        type: 'complete',
        buffer,
        usedBytes,
        frames: this.frames,
        limitReached,
      },
      [buffer],
    );
  }

  process(inputs, outputs) {
    // A connected output keeps the graph alive but never monitors the input.
    for (const output of outputs) {
      for (const channel of output) channel.fill(0);
    }
    if (this.terminal) return true;

    const input = inputs[0];
    // AudioWorklet supplies an empty channel array while the input is inactive.
    if (Array.isArray(input) && input.length === 0) return true;
    if (!input || input.length !== 1 || !(input[0] instanceof Float32Array)) {
      this.error('invalid_audio');
      return true;
    }
    const channel = input[0];
    if (channel.length === 0) {
      this.error('invalid_audio');
      return true;
    }

    const remaining = MAX_FRAMES - this.frames;
    const count = Math.min(channel.length, remaining);
    for (let i = 0; i < count; i++) {
      const sample = channel[i];
      if (!Number.isFinite(sample)) {
        this.error('invalid_audio');
        return true;
      }
      const clamped = Math.max(-1, Math.min(1, sample));
      const pcm = Math.round(clamped * (clamped < 0 ? 32768 : 32767));
      this.view.setInt16(WAV_HEADER_BYTES + (this.frames + i) * 2, pcm, true);
    }
    this.frames += count;
    if (this.frames === MAX_FRAMES) this.complete(true);
    return true;
  }
}

registerProcessor('anima-pcm-capture-v1', AnimaPcmCaptureProcessor);
