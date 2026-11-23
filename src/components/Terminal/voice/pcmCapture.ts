export const PCM_SAMPLE_RATE = 16_000;

// AudioWorklet modules load from blob URLs and cannot import, so worklets embed this source.
// iOS Safari and Bluetooth headsets run the context at the hardware rate (often 48 or 44.1 kHz).
export const PCM_RESAMPLER_SOURCE = `
class PcmResampler {
  constructor(inputRate, outputRate) {
    this.step = inputRate / outputRate;
    this.position = 0;
    this.previous = 0;
  }
  push(input, emit) {
    const length = input.length;
    if (!length) return;
    if (this.step === 1) {
      for (let index = 0; index < length; index++) emit(input[index]);
      return;
    }
    let position = this.position;
    while (position < length - 1) {
      const index = Math.floor(position);
      const before = index < 0 ? this.previous : input[index];
      emit(before + (input[index + 1] - before) * (position - index));
      position += this.step;
    }
    this.previous = input[length - 1];
    this.position = position - length;
  }
}
`;

// Prefer a 16 kHz context so desktop browsers resample natively; fall back to the hardware rate.
export function openCaptureContext(): AudioContext {
  let audio: AudioContext;
  try { audio = new AudioContext({ sampleRate: PCM_SAMPLE_RATE }); }
  catch { audio = new AudioContext(); }
  // iOS Safari unlocks audio only from resume() called synchronously inside the user gesture.
  void audio.resume().catch(() => {});
  return audio;
}
