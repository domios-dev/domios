import { useEffect, useRef, useState } from "react";
import { createDictationTransport, DICTATION_LIMIT_MS, DICTATION_PCM_BYTES } from "../../../lib/transport/dictation";
import { uid } from "../../../lib/uid";
import { useLatestCallback } from "../../../hooks/useLatestCallback";
import { claimCaptureOwnership, releaseCaptureOwnership } from "./localRecordingCaptureModel";
import type { DictationUpdate } from "./dictationSessionModel";
import { clearDictationSetupHandoff } from "./DictationSetupPrompt";
import { createDictationLevelChannel } from "./dictationLevelModel";
import { microphoneErrorMessage } from "./audio";
import { openCaptureContext, PCM_RESAMPLER_SOURCE, PCM_SAMPLE_RATE } from "./pcmCapture";

const QUEUE_LIMIT = 12;
const LEVEL_SAMPLES = 1120;
const WORKLET = `
const FRAME_BYTES = ${DICTATION_PCM_BYTES};
const LEVEL_SAMPLES = ${LEVEL_SAMPLES};
${PCM_RESAMPLER_SOURCE}
class DictationCapture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.frame = new ArrayBuffer(FRAME_BYTES);
    this.samples = new DataView(this.frame);
    this.length = 0;
    this.sent = 0;
    this.levelSum = 0;
    this.levelCount = 0;
    this.active = true;
    this.resampler = new PcmResampler(sampleRate, ${PCM_SAMPLE_RATE});
    this.write = this.write.bind(this);
    this.port.onmessage = ({data}) => {
      if (data === "ack") this.sent--;
      if (data === "finish") {
        this.active = false;
        if (this.length) this.send(this.frame.slice(0, this.length * 2));
        this.port.postMessage("finished");
      }
    };
  }
  send(frame) {
    if (this.sent >= 2) {
      this.active = false;
      this.port.postMessage("overflow");
      return;
    }
    this.sent++;
    this.port.postMessage(frame, [frame]);
  }
  write(sample) {
    if (!this.active) return;
    this.levelSum += sample * sample;
    if (++this.levelCount >= LEVEL_SAMPLES) {
      this.port.postMessage(Math.sqrt(this.levelSum / this.levelCount));
      this.levelSum = 0;
      this.levelCount = 0;
    }
    this.samples.setInt16(this.length++ * 2, Math.round(Math.max(-1, Math.min(1, sample)) * 32767), true);
    if (this.length * 2 === this.frame.byteLength) {
      this.send(this.frame);
      if (!this.active) return;
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

type Phase = "idle" | "starting" | "listening" | "finishing";
type Transport = ReturnType<typeof createDictationTransport>;
type DraftPort = {
  prepare(session: string): boolean;
  update(update: DictationUpdate): void;
  cancel(): void;
  composing(): boolean;
};

interface Capture {
  identity: string;
  transport: Transport;
  controller: AbortController;
  audio?: AudioContext;
  stream?: MediaStream;
  source?: MediaStreamAudioSourceNode;
  worklet?: AudioWorkletNode;
  timer?: number;
  permissionPending: boolean;
  claimed: boolean;
  stopping: boolean;
  startupDone: Promise<void>;
  finish: (() => void) | null;
  tail: Promise<void>;
  waiting: number;
  sequence: number;
  afterFinish?: () => void;
}

function release(capture: Capture) {
  if (capture.timer !== undefined) window.clearTimeout(capture.timer);
  capture.worklet?.disconnect();
  capture.worklet?.port.close();
  capture.source?.disconnect();
  capture.stream?.getTracks().forEach(track => track.stop());
  const audio = capture.audio;
  capture.audio = undefined;
  if (audio) void audio.close().catch(() => {});
  if (capture.claimed && !capture.permissionPending) {
    capture.claimed = false;
    releaseCaptureOwnership("manual");
  }
}

export function useLiveDictation(tabId: string, identity: string, enabled: boolean, draft: DraftPort) {
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState<string | null>(null);
  const [setup, setSetup] = useState<string | null>(null);
  const levelChannel = useRef(createDictationLevelChannel()).current;
  const owned = useRef<Capture | null>(null);
  const updates = useRef<DictationUpdate[]>([]);
  const deferredIdentity = useRef<string | null>(null);
  const rollback = useRef(false);
  const afterComposition = useRef<{ identity: string; callback: () => void } | undefined>(undefined);
  const port = useRef(draft);
  port.current = draft;
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const identityRef = useRef(identity);
  identityRef.current = identity;
  const pending = useRef(false);

  const compositionEnded = () => {
    if (rollback.current || (updates.current.length > 0
      && (!enabledRef.current || deferredIdentity.current !== identityRef.current))) port.current.cancel();
    else for (const update of updates.current) port.current.update(update);
    updates.current = [];
    deferredIdentity.current = null;
    rollback.current = false;
    const after = afterComposition.current;
    afterComposition.current = undefined;
    if (enabledRef.current && after?.identity === identityRef.current) after.callback();
  };
  const discard = useLatestCallback((capture: Capture, reason?: string): Promise<void> => {
    if (owned.current !== capture) return Promise.resolve();
    owned.current = null;
    capture.controller.abort();
    capture.finish?.();
    release(capture);
    const cancelled = capture.transport.cancel().catch(() => {});
    updates.current = [];
    deferredIdentity.current = null;
    afterComposition.current = undefined;
    if (port.current.composing()) rollback.current = true;
    else port.current.cancel();
    levelChannel.reset();
    setPhase("idle");
    if (reason) setError(reason);
    return Promise.all([cancelled, capture.startupDone]).then(() => undefined);
  }, []);
  function fail(capture: Capture, failure: unknown) {
    discard(capture, microphoneErrorMessage(failure));
  }

  useEffect(() => {
    const cancel = () => { if (owned.current) discard(owned.current); };
    const visibility = () => { if (document.visibilityState === "hidden") cancel(); };
    if (!enabled) cancel();
    window.addEventListener("pagehide", cancel);
    document.addEventListener("visibilitychange", visibility);
    return () => {
      window.removeEventListener("pagehide", cancel);
      document.removeEventListener("visibilitychange", visibility);
      cancel();
    };
  }, [identity, enabled, discard]);

  function publish(capture: Capture, update: DictationUpdate) {
    if (owned.current !== capture) return;
    if (!enabledRef.current || identityRef.current !== capture.identity) { discard(capture); return; }
    if (port.current.composing()) {
      if (updates.current.length === 32) throw new Error("Dictation paused too long during text composition.");
      deferredIdentity.current = capture.identity;
      updates.current.push(update);
    } else port.current.update(update);
  }

  async function finalize(capture: Capture) {
    await capture.tail;
    if (owned.current !== capture) return;
    const signal = AbortSignal.any([capture.controller.signal, AbortSignal.timeout(20_000)]);
    publish(capture, await capture.transport.finish(++capture.sequence, signal));
    if (owned.current !== capture) return;
    owned.current = null;
    levelChannel.reset();
    setPhase("idle");
    if (enabledRef.current && capture.afterFinish) {
      if (port.current.composing()) afterComposition.current = { identity: capture.identity, callback: capture.afterFinish };
      else capture.afterFinish();
    }
  }

  async function stop() {
    const capture = owned.current;
    if (!capture || capture.stopping) return;
    if (!capture.worklet) {
      capture.stopping = true;
      levelChannel.reset();
      setPhase("finishing");
      return;
    }
    capture.stopping = true;
    levelChannel.reset();
    setPhase("finishing");
    try {
      const flushed = new Promise<void>(resolve => { capture.finish = resolve; });
      capture.worklet.port.postMessage("finish");
      const deadline = AbortSignal.timeout(3_000);
      await Promise.race([flushed, new Promise<never>((_, reject) => {
        deadline.addEventListener("abort", () => reject(new Error("The microphone did not finish.")), { once: true });
      })]);
      release(capture);
      await finalize(capture);
    } catch (failure) { fail(capture, failure); }
  }

  async function start(prepare = port.current.prepare) {
    if (!enabledRef.current || owned.current || pending.current || updates.current.length || rollback.current || port.current.composing()) return;
    const session = uid();
    let transport: Transport;
    let audio: AudioContext;
    setError(null);
    setSetup(null);
    try {
      transport = createDictationTransport(session, tabId);
      audio = openCaptureContext();
    } catch (failure) { setError(microphoneErrorMessage(failure)); return; }
    if (!prepare(session)) { void audio.close().catch(() => {}); return; }
    let resolveStartup!: () => void;
    const startupDone = new Promise<void>(resolve => { resolveStartup = resolve; });
    const capture: Capture = { identity, transport, controller: new AbortController(), permissionPending: false, claimed: false,
      stopping: false, startupDone, finish: null, tail: Promise.resolve(), waiting: 0, sequence: 0, audio };
    owned.current = capture;
    pending.current = true;
    setPhase("starting");
    try {
      const result = await transport.start();
      if (owned.current !== capture) { if (result.state === "ready") await transport.cancel().catch(() => {}); return; }
      if (!enabledRef.current || identityRef.current !== capture.identity) { discard(capture); return; }
      if (result.state !== "ready") {
        discard(capture);
        if (result.state === "setup") setSetup(result.reason);
        else setError(result.reason);
        return;
      }
      clearDictationSetupHandoff();
      if (capture.stopping) { release(capture); await finalize(capture); return; }
      if (!navigator.mediaDevices?.getUserMedia) throw new Error("Microphone access needs a secure Domios window.");
      if (!claimCaptureOwnership("manual")) throw new Error("Another voice capture is already active.");
      capture.claimed = true;
      capture.permissionPending = true;
      try { capture.stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1 } }); }
      finally { capture.permissionPending = false; }
      if (owned.current !== capture || capture.stopping) {
        release(capture);
        if (owned.current === capture) await finalize(capture);
        return;
      }
      for (const track of capture.stream.getAudioTracks()) track.addEventListener("ended", () => {
        fail(capture, new Error("Microphone access was lost."));
      }, { once: true });
      if (capture.audio !== audio) return;
      const url = URL.createObjectURL(new Blob([WORKLET], { type: "text/javascript" }));
      try { await audio.audioWorklet.addModule(url); }
      finally { URL.revokeObjectURL(url); }
      if (owned.current !== capture) return;
      if (capture.stopping) { release(capture); await finalize(capture); return; }
      const source = audio.createMediaStreamSource(capture.stream);
      const worklet = new AudioWorkletNode(audio, "dictation-capture", { outputChannelCount: [1] });
      capture.source = source;
      capture.worklet = worklet;
      worklet.port.onmessage = ({ data }: MessageEvent<unknown>) => {
        if (owned.current !== capture) return;
        if (typeof data === "number") {
          if (!capture.stopping) levelChannel.pushRms(data);
          return;
        }
        if (data === "finished") { capture.finish?.(); return; }
        if (!(data instanceof ArrayBuffer) || capture.waiting === QUEUE_LIMIT) {
          fail(capture, new Error("Dictation could not keep up. Try a shorter recording."));
          return;
        }
        capture.waiting++;
        worklet.port.postMessage("ack");
        capture.tail = capture.tail.then(async () => {
          if (owned.current !== capture) return;
          const signal = AbortSignal.any([capture.controller.signal, AbortSignal.timeout(20_000)]);
          publish(capture, await transport.frame(++capture.sequence, data, signal));
          capture.waiting--;
        }).catch(failure => fail(capture, failure));
      };
      worklet.onprocessorerror = () => fail(capture, new Error("Microphone audio processing stopped."));
      source.connect(worklet).connect(audio.destination);
      await audio.resume();
      if (owned.current !== capture || capture.stopping) return;
      capture.timer = window.setTimeout(() => void stop(), DICTATION_LIMIT_MS);
      setPhase("listening");
    } catch (failure) {
      if (owned.current !== capture) release(capture);
      else fail(capture, failure);
    } finally {
      pending.current = false;
      resolveStartup();
    }
  }

  return { phase, error, setup, start, stop, compositionEnded,
    readLevel: levelChannel.read,
    subscribeLevel: levelChannel.subscribe,
    finishThen(after: () => void) {
      const capture = owned.current;
      if (!capture || capture.afterFinish) return;
      capture.afterFinish = after;
      void stop();
    },
    active: () => owned.current !== null || updates.current.length > 0 || rollback.current,
    cancel: (): void | Promise<void> => (owned.current ? discard(owned.current) : undefined),
    dismissSetup: () => setSetup(null),
    dismissError: () => setError(null),
  };
}

export type LiveDictation = ReturnType<typeof useLiveDictation>;
