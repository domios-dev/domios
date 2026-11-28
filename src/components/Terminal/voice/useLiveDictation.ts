import { useEffect, useRef, useState } from "react";
import { createDictationTransport } from "../../../lib/transport/dictation";
import { uid } from "../../../lib/uid";
import { useLatestCallback } from "../../../hooks/useLatestCallback";
import { claimCaptureOwnership, releaseCaptureOwnership } from "./localRecordingCaptureModel";
import type { DictationUpdate } from "./dictationSessionModel";
import { clearDictationSetupHandoff } from "./DictationSetupPrompt";
import { createDictationLevelChannel } from "./dictationLevelModel";
import { microphoneErrorMessage } from "./audio";
import { openCaptureContext } from "./pcmCapture";
import { DICTATION_BUFFER_MS, DICTATION_WORKLET_SOURCE } from "./dictationWorklet";

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
  endReason?: string;
  permissionPending: boolean;
  claimed: boolean;
  stopping: boolean;
  startupDone: Promise<void>;
  finish: (() => void) | null;
  tail: Promise<void>;
  sequence: number;
  afterFinish?: () => void;
}

function release(capture: Capture) {
  capture.worklet?.disconnect();
  capture.worklet?.port.close();
  capture.source?.disconnect();
  capture.stream?.getTracks().forEach(track => track.stop());
  capture.worklet = undefined;
  capture.source = undefined;
  capture.stream = undefined;
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
    if (enabledRef.current && deferredIdentity.current === identityRef.current) {
      for (const update of updates.current) port.current.update(update);
    }
    if (rollback.current) port.current.cancel();
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
    if (!enabledRef.current || identityRef.current !== capture.identity) {
      updates.current = [];
      deferredIdentity.current = null;
    }
    afterComposition.current = undefined;
    rollback.current = true;
    if (!port.current.composing()) compositionEnded();
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
      deferredIdentity.current = capture.identity;
      updates.current.push(update);
    } else port.current.update(update);
  }

  async function finalize(capture: Capture) {
    await capture.tail;
    if (owned.current !== capture) return;
    const { update, stopReason } = await capture.transport.finish(++capture.sequence, frameSignal(capture));
    publish(capture, update);
    capture.endReason ??= stopReason;
    if (owned.current !== capture) return;
    owned.current = null;
    levelChannel.reset();
    setPhase("idle");
    if (capture.endReason) setError(capture.endReason);
    else if (enabledRef.current && capture.afterFinish) {
      if (port.current.composing()) afterComposition.current = { identity: capture.identity, callback: capture.afterFinish };
      else capture.afterFinish();
    }
  }

  function frameSignal(capture: Capture) {
    return AbortSignal.any([capture.controller.signal, AbortSignal.timeout(DICTATION_BUFFER_MS + 100_000)]);
  }

  async function stopCapture(capture: Capture, reason?: string, workletFinished = false) {
    if (owned.current !== capture) return;
    capture.endReason ??= reason;
    if (workletFinished) capture.finish?.();
    if (capture.stopping) return;
    capture.stopping = true;
    levelChannel.reset();
    setPhase("finishing");
    if (!capture.worklet) return;
    if (!workletFinished) {
      const port = capture.worklet.port;
      const flushed = new Promise<void>(resolve => { capture.finish = resolve; });
      try {
        port.postMessage("finish");
        const deadline = AbortSignal.timeout(DICTATION_BUFFER_MS);
        await Promise.race([flushed, new Promise<never>((_, reject) => {
          deadline.addEventListener("abort", () => reject(new Error("The microphone did not finish.")), { once: true });
        })]);
      } catch (failure) {
        capture.endReason ??= microphoneErrorMessage(failure);
      }
    }
    release(capture);
    try { await finalize(capture); }
    catch (failure) { fail(capture, failure); }
  }

  async function stop() {
    if (owned.current) await stopCapture(owned.current);
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
      stopping: false, startupDone, finish: null, tail: Promise.resolve(), sequence: 0, audio };
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
        void stopCapture(capture, "Microphone access was lost.");
      }, { once: true });
      if (capture.audio !== audio) return;
      const url = URL.createObjectURL(new Blob([DICTATION_WORKLET_SOURCE], { type: "text/javascript" }));
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
          worklet.port.postMessage("levelAck");
          return;
        }
        if (data === "finished") { capture.finish?.(); return; }
        if (data === "overflow") {
          void stopCapture(capture, "Dictation stopped because the audio buffer filled.", true);
          return;
        }
        if (!(data instanceof ArrayBuffer)) return;
        // The worklet holds the credit until this frame leaves the transcription queue.
        capture.tail = capture.tail.then(async () => {
          if (owned.current !== capture) return;
          const { update, stopReason } = await transport.frame(++capture.sequence, data, frameSignal(capture));
          publish(capture, update);
          if (stopReason) throw new Error(stopReason);
          if (owned.current === capture) capture.worklet?.port.postMessage("ack");
        }).catch(failure => fail(capture, failure));
      };
      worklet.onprocessorerror = () => {
        void stopCapture(capture, "Microphone audio processing stopped.", true);
      };
      source.connect(worklet).connect(audio.destination);
      await audio.resume();
      if (owned.current !== capture || capture.stopping) return;
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
