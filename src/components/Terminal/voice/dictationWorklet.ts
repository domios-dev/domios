import { DICTATION_PCM_BYTES } from "../../../lib/transport/dictation";
import { PCM_RESAMPLER_SOURCE, PCM_SAMPLE_RATE } from "./pcmCapture";

export const DICTATION_BUFFER_MS = 180_000;
const FRAME_MS = DICTATION_PCM_BYTES * 1000 / (PCM_SAMPLE_RATE * 2);
const BUFFER_FRAMES = DICTATION_BUFFER_MS / FRAME_MS;
const LEVEL_SAMPLES = 1120;
export const DICTATION_WORKLET_SOURCE = `
const FRAME_BYTES = ${DICTATION_PCM_BYTES};
const LEVEL_SAMPLES = ${LEVEL_SAMPLES};
const BUFFER_FRAMES = ${BUFFER_FRAMES};
${PCM_RESAMPLER_SOURCE}
class DictationCapture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.frame = new ArrayBuffer(FRAME_BYTES);
    this.samples = new DataView(this.frame);
    this.length = 0;
    this.sent = 0;
    this.levelPending = false;
    this.levelSum = 0;
    this.levelCount = 0;
    this.active = true;
    this.resampler = new PcmResampler(sampleRate, ${PCM_SAMPLE_RATE});
    this.write = this.write.bind(this);
    this.port.onmessage = ({data}) => {
      if (data === "ack") this.sent--;
      if (data === "levelAck") this.levelPending = false;
      if (data === "finish" && this.active) {
        this.active = false;
        if (this.length) this.send(this.frame.slice(0, this.length * 2));
        this.port.postMessage("finished");
      }
    };
  }
  send(frame) {
    this.sent++;
    this.port.postMessage(frame, [frame]);
  }
  write(sample) {
    if (!this.active) return;
    this.levelSum += sample * sample;
    if (++this.levelCount >= LEVEL_SAMPLES) {
      if (!this.levelPending) {
        this.levelPending = true;
        this.port.postMessage(Math.sqrt(this.levelSum / this.levelCount));
      }
      this.levelSum = 0;
      this.levelCount = 0;
    }
    this.samples.setInt16(this.length++ * 2, Math.round(Math.max(-1, Math.min(1, sample)) * 32767), true);
    if (this.length * 2 === this.frame.byteLength) {
      this.send(this.frame);
      if (this.sent > BUFFER_FRAMES) {
        this.active = false;
        this.port.postMessage("overflow");
        return;
      }
      this.frame = new ArrayBuffer(FRAME_BYTES);
      this.samples = new DataView(this.frame);
      this.length = 0;
    }
  }
  process(inputs) {
    if (!this.active) return true;
    const audio = inputs[0]?.[0];
    if (audio) this.resampler.push(audio, this.write);
    return true;
  }
}
registerProcessor("dictation-capture", DictationCapture);
`;
