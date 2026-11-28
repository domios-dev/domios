import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createDictationTransport, DICTATION_PCM_BYTES } from "../../../lib/transport/dictation";
import { DictationSessionModel } from "./dictationSessionModel";
import { DICTATION_WORKLET_SOURCE } from "./dictationWorklet";
import { PCM_SAMPLE_RATE } from "./pcmCapture";
import { useLiveDictation, type LiveDictation } from "./useLiveDictation";

vi.mock("../../../lib/transport/dictation", () => ({
  createDictationTransport: vi.fn(),
  DICTATION_PCM_BYTES: 16_000,
}));

type WorkletMessage = ArrayBuffer | string | number;

function captureWorklet() {
  const messages: WorkletMessage[] = [];
  const port = {
    onmessage: null as ((event: { data: string }) => void) | null,
    postMessage(data: WorkletMessage) { messages.push(data); },
  };
  class Processor {
    port = port;
  }
  type Capture = { process(inputs: Float32Array[][]): void };
  let capture!: Capture;
  const register = (_name: string, constructor: new () => Capture) => { capture = new constructor(); };
  new Function("AudioWorkletProcessor", "sampleRate", "registerProcessor", DICTATION_WORKLET_SOURCE)(
    Processor, PCM_SAMPLE_RATE, register,
  );
  return {
    render(audio: Float32Array) { capture.process([[audio]]); },
    command(data: string) { port.onmessage?.({ data }); },
    takeMessages() { return messages.splice(0); },
  };
}

afterEach(() => vi.unstubAllGlobals());

it("drains two minutes of stalled audio in order and retains queued commits on failure", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const worklet = captureWorklet();
  const mainPort = {
    onmessage: null as ((event: { data: WorkletMessage }) => void) | null,
    postMessage: worklet.command,
    close() {},
  };
  const deliver = (data: WorkletMessage) => mainPort.onmessage?.({ data });
  class AudioNode {
    connect<T>(node: T) { return node; }
    disconnect() {}
  }
  vi.stubGlobal("AudioWorkletNode", class extends AudioNode { port = mainPort; });
  vi.stubGlobal("AudioContext", class {
    audioWorklet = { addModule: async () => {} };
    destination = new AudioNode();
    close = async () => {};
    resume = async () => {};
    createMediaStreamSource = () => new AudioNode();
  });
  vi.stubGlobal("URL", class extends URL {
    static createObjectURL = () => "blob:microphone";
    static revokeObjectURL = () => {};
  });
  const getUserMedia = async () => ({ getAudioTracks: () => [], getTracks: () => [] });
  vi.stubGlobal("navigator", Object.create(navigator, { mediaDevices: { value: { getUserMedia } } }));
  const frames = 240;
  const samples = new Float32Array(DICTATION_PCM_BYTES / 2);
  const expectedSamples: number[] = [];
  const receivedSamples: number[] = [];
  const cancel = vi.fn(async () => {});
  let completeFirstFrame!: () => void;
  const firstFrame = new Promise<void>(resolve => { completeFirstFrame = resolve; });
  const draftText = () => Array.from({ length: frames }, (_, index) => `word${index + 1}`).join(" ");
  vi.mocked(createDictationTransport).mockImplementation(session => ({
    start: async () => ({ state: "ready" }),
    frame: async (sequence, pcm) => {
      receivedSamples.push(new DataView(pcm).getInt16(0, true));
      if (sequence === 1) await firstFrame;
      return {
        update: { session, sequence, committed: `word${sequence}`, provisional: "preview", final: false },
        stopReason: sequence === frames ? "The transcription worker stopped." : undefined,
      };
    },
    finish: vi.fn(async () => { throw new Error("Unexpected finish"); }),
    cancel,
  }));
  let dictation!: LiveDictation;
  let model!: DictationSessionModel;
  let composing = false;
  function Harness() {
    dictation = useLiveDictation("tab", "identity", true, {
      prepare(session) {
        model = new DictationSessionModel(session, "", 0, 0);
        return true;
      },
      update: update => { model.update(update); },
      composing: () => composing,
      cancel: () => { model.cancel(); },
    });
    return null;
  }
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () => root.render(<Harness />));
    await act(async () => dictation.start());
    expect(dictation.phase).toBe("listening");

    for (let frame = 0; frame < frames; frame++) {
      samples.fill(frame / frames);
      expectedSamples.push(Math.round(samples[0] * 32767));
      worklet.render(samples);
    }
    const backlog = worklet.takeMessages();
    expect(backlog).not.toContain("overflow");
    expect(backlog.filter(message => message instanceof ArrayBuffer)).toHaveLength(frames);

    composing = true;
    await act(async () => { for (const message of backlog) deliver(message); });
    expect(receivedSamples).toEqual(expectedSamples.slice(0, 1));
    expect(dictation.phase).toBe("listening");
    expect(dictation.error).toBeNull();

    await act(async () => completeFirstFrame());
    expect(receivedSamples).toEqual(expectedSamples);
    expect(dictation.error).toBe("The transcription worker stopped.");
    expect(model.durable).toBe("");
    composing = false;
    await act(async () => dictation.compositionEnded());
    expect(model.durable).toBe(draftText());
    expect(model.text).toBe(draftText());
    expect(cancel).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
  }
});
