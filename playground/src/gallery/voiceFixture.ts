import { bubblePoseFromPointer } from "../../../src/components/Terminal/composerBubble";
import type { ComposerVoiceHandle } from "../../../src/components/Terminal/voice/useComposerVoice";

export type VoicePhase = "idle" | "hold" | "sliding" | "cancel" | "locked" | "preview" | "playing";

const PEAKS = [0.2, 0.46, 0.3, 0.68, 0.38, 0.82, 0.5, 0.27, 0.62, 0.4, 0.74, 0.32, 0.54, 0.25, 0.66, 0.4, 0.58, 0.3, 0.72, 0.44, 0.36, 0.6, 0.28, 0.5, 0.42, 0.64, 0.34, 0.48];
const noop = () => undefined;
const asyncNoop = async () => undefined;

function toneWav(seconds: number): Blob {
  const rate = 8000;
  const samples = rate * seconds;
  const buffer = new ArrayBuffer(44 + samples * 2);
  const view = new DataView(buffer);
  const text = (offset: number, value: string) => [...value].forEach((char, index) => view.setUint8(offset + index, char.charCodeAt(0)));
  text(0, "RIFF");
  view.setUint32(4, 36 + samples * 2, true);
  text(8, "WAVEfmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, rate, true);
  view.setUint32(28, rate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  text(36, "data");
  view.setUint32(40, samples * 2, true);
  for (let index = 0; index < samples; index += 1) view.setInt16(44 + index * 2, Math.sin(index / 6) * 800, true);
  return new Blob([buffer], { type: "audio/wav" });
}

const TONE = toneWav(12);
const TONE_URL = URL.createObjectURL(TONE);

export function voiceHandle(phase: VoicePhase, mainIsSend = false): ComposerVoiceHandle {
  const recording = phase === "hold" || phase === "sliding" || phase === "cancel" || phase === "locked";
  const preview = phase === "preview" || phase === "playing";
  const hold = phase === "hold" || phase === "sliding" || phase === "cancel";
  const voice = {
    recording,
    elapsed: 8.4,
    preview: preview ? { url: TONE_URL, durationSec: 12, peaks: PEAKS, blob: TONE, mime: "audio/wav" } : null,
    livePeaks: PEAKS.slice(0, 18),
    start: asyncNoop,
    stop: async () => [],
    stopForPreview: asyncNoop,
    commitPreview: async () => [],
    discardPreview: noop,
  } as unknown as ComposerVoiceHandle["voice"];
  return {
    voice,
    shellRef: { current: null },
    cancelHintRef: { current: null },
    buttonRef: { current: null },
    voiceMode: hold ? "hold" : phase === "locked" ? "locked" : preview ? "preview" : null,
    voicePressed: hold,
    voiceBusy: false,
    bubblePose: phase === "sliding" || phase === "cancel" ? bubblePoseFromPointer(phase === "cancel" ? -140 : -60, 0, 0, 0) : { x: 0, y: 0, sx: 1, sy: 1 },
    cancelArmed: phase === "cancel",
    isHold: hold,
    isLocked: phase === "locked",
    isPreview: preview,
    voiceActive: hold || recording,
    mainIsSend,
    composerHidden: recording || preview,
    mainTitle: mainIsSend ? "Send" : phase === "locked" ? "Stop and review voice note" : "Hold to record voice note",
    startVoice: asyncNoop,
    moveVoice: noop,
    cancelVoice: asyncNoop,
    stopToPreview: asyncNoop,
    sendPreview: asyncNoop,
    discardPreviewClip: noop,
    lockFromPointerCancel: noop,
  } as unknown as ComposerVoiceHandle;
}
