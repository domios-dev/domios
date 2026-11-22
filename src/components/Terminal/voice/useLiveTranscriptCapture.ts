import { useEffect, useRef, useState } from "react";
import { useLatestCallback } from "../../../hooks/useLatestCallback";
import { DICTATION_PCM_BYTES } from "../../../lib/transport/dictation";
import {
  createLiveTranscriptCaptureTransport,
  type LiveCaptureAdmission,
  type LiveCapturePauseReason,
} from "../../../lib/transport/liveTranscriptCapture";
import { microphoneErrorMessage } from "./audio";
import { claimCaptureOwnership, releaseCaptureOwnership } from "./localRecordingCaptureModel";

const QUEUE_LIMIT = 24;
const WORKLET = `
class LiveTranscriptMicrophone extends AudioWorkletProcessor {
  constructor() {
    super();
    this.active = true;
    this.pending = 0;
    this.reset();
    this.port.onmessage = ({ data }) => {
      if (data === "ack") this.pending--;
      if (data === "seal") {
        if (this.active && this.length) this.send(this.buffer.slice(0, this.length * 2));
        this.active = false;
        this.port.postMessage("sealed");
      }
    };
  }
  reset() {
    this.buffer = new ArrayBuffer(${DICTATION_PCM_BYTES});
    this.view = new DataView(this.buffer);
    this.length = 0;
  }
  send(frame) {
    if (this.pending >= 2) {
      this.active = false;
      this.port.postMessage("overflow");
      return;
    }
    this.pending++;
    this.port.postMessage(frame, [frame]);
  }
  process(inputs) {
    if (!this.active) return true;
    for (const sample of inputs[0]?.[0] ?? []) {
      this.view.setInt16(this.length++ * 2, Math.round(Math.max(-1, Math.min(1, sample)) * 32767), true);
      if (this.length * 2 === this.buffer.byteLength) {
        this.send(this.buffer);
        if (!this.active) break;
        this.reset();
      }
    }
    return true;
  }
}
registerProcessor("live-transcript-microphone", LiveTranscriptMicrophone);
`;

type Transport = ReturnType<typeof createLiveTranscriptCaptureTransport>;
type CapturePhase = "idle" | "starting" | "recording" | "pausing" | "paused" | "setup" | "error";

interface CaptureOptions {
  tabId: string;
  identity: string;
  enabled: boolean;
  autoResume?: boolean;
}

interface Recording {
  tabId: string;
  identity: string;
  transport: Transport;
  admission: Promise<LiveCaptureAdmission>;
  io: AbortController;
  lease?: string;
  permissionPending: boolean;
  microphoneClaimed: boolean;
  stream?: MediaStream;
  audio?: AudioContext;
  source?: MediaStreamAudioSourceNode;
  worklet?: AudioWorkletNode;
  sealed?: () => void;
  sequence: number;
  queueSize: number;
  tail: Promise<void>;
  pauseReason?: LiveCapturePauseReason;
  failure?: string;
}

function stopTracks(recording: Recording) {
  const stream = recording.stream;
  recording.stream = undefined;
  stream?.getTracks().forEach(track => track.stop());
}

function release(recording: Recording) {
  stopTracks(recording);
  recording.worklet?.port.close();
  recording.worklet?.disconnect();
  recording.source?.disconnect();
  const audio = recording.audio;
  recording.audio = undefined;
  recording.worklet = undefined;
  recording.source = undefined;
  if (audio) void audio.close().catch(() => {});
  if (recording.microphoneClaimed && !recording.permissionPending) {
    recording.microphoneClaimed = false;
    releaseCaptureOwnership("manual");
  }
}

function sealWorklet(recording: Recording): Promise<void> {
  const worklet = recording.worklet;
  if (!worklet) return Promise.resolve();
  return new Promise(resolve => {
    const deadline = AbortSignal.timeout(2000);
    recording.sealed = () => {
      deadline.removeEventListener("abort", expired);
      recording.sealed = undefined;
      resolve();
    };
    function expired() {
      recording.failure ??= "The microphone did not deliver its final audio frame.";
      recording.sealed?.();
    }
    deadline.addEventListener("abort", expired, { once: true });
    worklet.port.postMessage("seal");
  });
}

async function openWorklet(recording: Recording): Promise<AudioContext | undefined> {
  const audio = new AudioContext({ sampleRate: 16_000 });
  recording.audio = audio;
  if (audio.sampleRate !== 16_000) throw new Error("The microphone cannot capture at 16 kHz.");
  const script = URL.createObjectURL(new Blob([WORKLET], { type: "text/javascript" }));
  try { await audio.audioWorklet.addModule(script); }
  finally { URL.revokeObjectURL(script); }
  return recording.pauseReason ? undefined : audio;
}

export function useLiveTranscriptCapture(options: CaptureOptions) {
  const { tabId, identity, enabled } = options;
  const [phase, setPhase] = useState<CapturePhase>("idle");
  const [error, setError] = useState<string | null>(null);
  const [provisional, setProvisional] = useState("");
  const [revision, setRevision] = useState(0);
  const owned = useRef<Recording | null>(null);
  const permission = useRef<Recording | null>(null);
  const resumable = useRef<Pick<Recording, "tabId" | "identity"> | null>(null);
  const autoPending = useRef(false);
  const mounted = useRef(false);
  const latest = useRef(options);
  latest.current = options;

  const start = useLatestCallback(async (resume?: boolean) => {
    if (!eligible() || owned.current || permission.current) return;
    setError(null);
    setProvisional("");
    let transport: Transport;
    try {
      transport = createLiveTranscriptCaptureTransport(tabId);
      if (!navigator.mediaDevices?.getUserMedia) throw new Error("Microphone access needs a secure Domios window.");
      if (!claimCaptureOwnership("manual")) throw new Error("Another microphone recording is already active.");
    } catch (failure) {
      setPhase("error");
      setError(microphoneErrorMessage(failure));
      return;
    }
    autoPending.current = false;
    const io = new AbortController();
    const recording: Recording = {
      tabId, identity, transport, io,
      admission: transport.start(resume ?? isSameTab(resumable.current), AbortSignal.timeout(40_000)),
      microphoneClaimed: true, permissionPending: false,
      sequence: 0, queueSize: 0, tail: Promise.resolve(),
    };
    owned.current = recording;
    setPhase("starting");
    try {
      const result = await recording.admission;
      if (recording.pauseReason || owned.current !== recording) return;
      if (result.state !== "ready") {
        retire(recording, result.state === "setup" ? "setup" : "error", result.reason);
        return;
      }
      recording.lease = result.lease;
      transport.assertCurrent();
      if (!eligible() || !isSameTab(recording)) { await pause("tabChanged"); return; }
      permission.current = recording;
      recording.permissionPending = true;
      let stream: MediaStream;
      try {
        stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1 } });
        recording.stream = stream;
      } finally {
        recording.permissionPending = false;
        permission.current = null;
        if (recording.pauseReason) {
          release(recording);
          resumeOnFocus();
        }
      }
      if (recording.pauseReason || owned.current !== recording) return;
      transport.assertCurrent();
      if (!eligible() || !isSameTab(recording)) { await pause("tabChanged"); return; }
      for (const track of stream.getAudioTracks()) {
        track.addEventListener("ended", () => {
          if (owned.current !== recording || recording.pauseReason) return;
          recording.failure = "Microphone access was lost.";
          void pause("microphoneLost");
        }, { once: true });
      }
      const audio = await openWorklet(recording);
      if (!audio || owned.current !== recording || recording.pauseReason) return;
      if (!eligible() || !isSameTab(recording)) { await pause("tabChanged"); return; }
      transport.assertCurrent();
      attachAudio(recording, audio, stream, result.lease);
      await audio.resume();
      if (owned.current === recording && !recording.pauseReason) transport.assertCurrent();
      if (owned.current === recording && !recording.pauseReason && mounted.current) setPhase("recording");
    } catch (failure) {
      if (owned.current === recording && !recording.pauseReason) {
        recording.failure = microphoneErrorMessage(failure);
        await pause("failed");
      }
    }
  }, []);

  const pause = useLatestCallback(async (reason: LiveCapturePauseReason = "manual") => {
    const recording = owned.current;
    if (!recording || recording.pauseReason) {
      if (reason === "manual") autoPending.current = false;
      return;
    }
    recording.pauseReason = reason;
    autoPending.current = reason === "focusLost" || reason === "hidden";
    stopTracks(recording);
    if (mounted.current) {
      setProvisional("");
      setPhase("pausing");
    }
    try {
      await sealWorklet(recording);
      release(recording);
      await recording.tail;
      if (owned.current !== recording) return;
      const admitted = await recording.admission;
      if (admitted.state === "ready") {
        recording.lease = admitted.lease;
        const marker = recording.failure && reason !== "microphoneLost" ? "failed" : reason;
        await recording.transport.pause(admitted.lease, ++recording.sequence, marker, AbortSignal.timeout(30_000));
        resumable.current = { tabId: recording.tabId, identity: recording.identity };
      }
      retire(recording, recording.failure ? "error" : "paused", recording.failure);
      if (!recording.failure) resumeOnFocus();
    } catch (failure) {
      fail(recording, failure);
    }
  }, []);

  function attachAudio(recording: Recording, audio: AudioContext, stream: MediaStream, lease: string) {
    const source = audio.createMediaStreamSource(stream);
    const worklet = new AudioWorkletNode(audio, "live-transcript-microphone", { outputChannelCount: [1] });
    recording.source = source;
    recording.worklet = worklet;
    worklet.onprocessorerror = () => {
      recording.failure = "Microphone audio processing stopped.";
      recording.sealed?.();
      if (owned.current === recording) void pause("failed");
    };
    worklet.port.onmessage = ({ data }: MessageEvent<unknown>) => {
      if (owned.current !== recording) return;
      if (data === "sealed") { recording.sealed?.(); return; }
      if (!(data instanceof ArrayBuffer) || recording.queueSize >= QUEUE_LIMIT) {
        recording.failure = "Live capture could not keep up with the microphone.";
        if (!recording.pauseReason) void pause("failed");
        return;
      }
      worklet.port.postMessage("ack");
      recording.queueSize++;
      recording.tail = recording.tail.then(async () => {
        if (owned.current !== recording) return;
        const signal = AbortSignal.any([recording.io.signal, AbortSignal.timeout(55_000)]);
        const update = await recording.transport.frame(lease, ++recording.sequence, data, signal);
        if (owned.current === recording && !recording.pauseReason && mounted.current) {
          setProvisional(update.provisional);
          setRevision(value => value + 1);
        }
        recording.queueSize--;
      }).catch(failure => fail(recording, failure));
    };
    source.connect(worklet).connect(audio.destination);
  }

  function fail(recording: Recording, failure: unknown) {
    if (owned.current !== recording) return;
    const detail = microphoneErrorMessage(failure);
    autoPending.current = false;
    recording.pauseReason ??= "failed";
    recording.io.abort();
    recording.sealed?.();
    retire(recording, "error", detail);
    if (recording.lease) void recording.transport.cancel(recording.lease).catch(() => {});
  }

  function retire(recording: Recording, next: CapturePhase, message?: string) {
    if (owned.current !== recording) return;
    release(recording);
    owned.current = null;
    if (mounted.current) {
      setProvisional("");
      setPhase(next);
      setError(message ?? null);
    }
  }

  function isSameTab(previous: Pick<Recording, "tabId" | "identity"> | null) {
    return previous?.identity === latest.current.identity && previous.tabId === latest.current.tabId;
  }

  function eligible() {
    return latest.current.enabled && mounted.current &&
      (!latest.current.autoResume || (document.visibilityState !== "hidden" && document.hasFocus()));
  }

  const resumeOnFocus = useLatestCallback(() => {
    if (latest.current.autoResume && autoPending.current && isSameTab(resumable.current)
      && eligible() && !owned.current && !permission.current) void start(true);
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  useEffect(() => {
    const hidden = () => {
      if (document.visibilityState === "hidden") {
        if (latest.current.autoResume) void pause("hidden");
      } else resumeOnFocus();
    };
    const pageHide = () => { void pause("hidden"); };
    const blur = () => { if (latest.current.autoResume) void pause("focusLost"); };
    window.addEventListener("blur", blur);
    window.addEventListener("focus", resumeOnFocus);
    window.addEventListener("pagehide", pageHide);
    document.addEventListener("visibilitychange", hidden);
    return () => {
      window.removeEventListener("blur", blur);
      window.removeEventListener("focus", resumeOnFocus);
      window.removeEventListener("pagehide", pageHide);
      document.removeEventListener("visibilitychange", hidden);
      autoPending.current = false;
      void pause("tabChanged");
    };
  }, [tabId, identity, enabled, pause, resumeOnFocus]);

  return { phase, error, provisional, revision, start, pause };
}
