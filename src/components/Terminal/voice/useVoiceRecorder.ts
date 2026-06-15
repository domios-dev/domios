import { useEffect, useRef, useState } from "react";
import { IS_WEB } from "../../../lib/webApi";
import {
  MIN_VOICE_BYTES,
  audioExt,
  microphoneErrorMessage,
  pickAudioMime,
  storeBlob,
} from "./audio";

export interface VoicePreviewData {
  url: string;
  durationSec: number;
  peaks: number[];
  blob: Blob;
  mime: string;
}

const WAVEFORM_BARS = 40;
const LIVE_WAVEFORM_BARS = 24;

function quietLiveWaveform(): number[] {
  return new Array(LIVE_WAVEFORM_BARS).fill(0.1);
}

/** Downsample the raw per-tick peaks captured while recording into a fixed
 * bar count, normalised to 0..1 so the preview waveform fills its track. */
function buildWaveform(raw: number[]): number[] {
  if (raw.length === 0) return new Array(WAVEFORM_BARS).fill(0.15);
  const bars: number[] = [];
  const step = raw.length / WAVEFORM_BARS;
  for (let i = 0; i < WAVEFORM_BARS; i++) {
    const from = Math.floor(i * step);
    const to = Math.max(from + 1, Math.floor((i + 1) * step));
    let peak = 0;
    for (let j = from; j < to && j < raw.length; j++) peak = Math.max(peak, raw[j]);
    bars.push(peak);
  }
  const max = Math.max(...bars, 0.001);
  return bars.map((b) => Math.max(0.12, b / max));
}

type StopKind = "send" | "discard" | "preview";

/**
 * Telegram-style voice capture. `stop()` uploads and returns the attachment
 * path (quick hold-release send). `stopForPreview()` keeps the clip local so
 * the user can listen first; `commitPreview()` then uploads it, or
 * `discardPreview()` drops it. A live AnalyserNode records amplitude peaks for
 * the preview waveform.
 */
export function useVoiceRecorder(onError: (msg: string) => void) {
  const [recording, setRecording] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [preview, setPreview] = useState<VoicePreviewData | null>(null);
  const [livePeaks, setLivePeaks] = useState<number[]>(quietLiveWaveform);

  const recRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const streamRef = useRef<MediaStream | null>(null);
  const stopResolveRef = useRef<((result: string[] | boolean) => void) | null>(null);
  const stopKindRef = useRef<StopKind>("send");
  const startedAtRef = useRef(0);
  const mimeRef = useRef<string | undefined>(undefined);
  const recDebugRef = useRef({
    chunks: 0,
    bytes: 0,
    lastChunkBytes: 0,
    lastChunkAt: 0,
  });

  // Live waveform sampling.
  const audioCtxRef = useRef<AudioContext | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const peaksRef = useRef<number[]>([]);
  const sampleTimerRef = useRef<number | null>(null);

  useEffect(() => {
    if (!recording) return;
    setElapsed(0);
    const t = window.setInterval(() => setElapsed((s) => s + 1), 1000);
    return () => window.clearInterval(t);
  }, [recording]);

  useEffect(
    () => () => {
      recRef.current?.stop();
      streamRef.current?.getTracks().forEach((t) => t.stop());
      teardownAnalyser();
      setPreview((p) => {
        if (p) URL.revokeObjectURL(p.url);
        return null;
      });
    },
    [],
  );

  function teardownAnalyser() {
    if (sampleTimerRef.current != null) {
      window.clearInterval(sampleTimerRef.current);
      sampleTimerRef.current = null;
    }
    analyserRef.current = null;
    void audioCtxRef.current?.close().catch(() => {});
    audioCtxRef.current = null;
  }

  // iOS standalone PWAs re-prompt for mic access on EVERY getUserMedia call —
  // they don't persist the grant the way Safari tabs do. So acquire the stream
  // ONCE and keep it warm: reuse it across recordings, only stopping the tracks
  // on real teardown (unmount). The mic-in-use indicator stays on between
  // takes, which is the accepted trade for not re-prompting every ~1-2 min.
  async function acquireWarmStream(): Promise<MediaStream> {
    const warm = streamRef.current;
    if (warm && warm.getAudioTracks().some((t) => t.readyState === "live")) {
      return warm;
    }
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    streamRef.current = stream;
    return stream;
  }

  function startSampling(stream: MediaStream) {
    try {
      const Ctx = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!Ctx) return;
      const ctx = new Ctx();
      const source = ctx.createMediaStreamSource(stream);
      const analyser = ctx.createAnalyser();
      analyser.fftSize = 512;
      source.connect(analyser);
      audioCtxRef.current = ctx;
      analyserRef.current = analyser;
      peaksRef.current = [];
      const buf = new Uint8Array(analyser.fftSize);
      sampleTimerRef.current = window.setInterval(() => {
        analyser.getByteTimeDomainData(buf);
        let peak = 0;
        for (let i = 0; i < buf.length; i++) peak = Math.max(peak, Math.abs(buf[i] - 128) / 128);
        peaksRef.current.push(peak);
        setLivePeaks((prev) => [...prev.slice(1), Math.max(0.08, Math.min(1, peak * 3.2))]);
      }, 60);
    } catch {
      // Waveform is decorative — never let analyser setup break recording.
    }
  }

  function finalizeBlob(): { blob: Blob; mime: string } | null {
    const mime = recRef.current?.mimeType || mimeRef.current || "audio/webm";
    const blob = new Blob(chunksRef.current, { type: mime });
    const debug = recDebugRef.current;
    console.info("[voice] recorder finalized", {
      mime,
      blobBytes: blob.size,
      chunkCount: debug.chunks,
      chunkBytes: debug.bytes,
      lastChunkBytes: debug.lastChunkBytes,
      msSinceLastChunk: debug.lastChunkAt > 0 ? Math.round(performance.now() - debug.lastChunkAt) : null,
    });
    chunksRef.current = [];
    if (blob.size < MIN_VOICE_BYTES) return null;
    return { blob, mime };
  }

  function describeAudioTracks(stream: MediaStream) {
    return stream.getAudioTracks().map((track) => ({
      id: track.id,
      label: track.label,
      enabled: track.enabled,
      muted: track.muted,
      readyState: track.readyState,
      settings: track.getSettings?.(),
    }));
  }

  function reportEmptyRecording(kind: StopKind, durationSec: number) {
    console.warn("[voice] recorder produced no usable audio", {
      kind,
      durationSec,
      minBytes: MIN_VOICE_BYTES,
      debug: recDebugRef.current,
      tracks: streamRef.current ? describeAudioTracks(streamRef.current) : [],
    });
    if (kind !== "discard") {
      onError("Voice recording produced no usable audio. Check microphone access and try again.");
    }
  }

  function createRecorder(stream: MediaStream, preferredMime: string | undefined): MediaRecorder {
    if (!preferredMime) return new MediaRecorder(stream);
    try {
      return new MediaRecorder(stream, { mimeType: preferredMime });
    } catch (e) {
      console.warn("[voice] preferred recorder mime failed, falling back to browser default", {
        preferredMime,
        error: e instanceof Error ? e.message : String(e),
      });
      mimeRef.current = undefined;
      return new MediaRecorder(stream);
    }
  }

  async function start(): Promise<boolean> {
    if (!navigator.mediaDevices?.getUserMedia) {
      onError(
        IS_WEB
          ? "Voice needs HTTPS on phones. Open Mesh -> Web UI and scan/copy the https://*.ts.net link; http://100.x cannot use the mic."
          : "Microphone capture is unavailable in this CodeTerm window. Restart the app after updating microphone permissions.",
      );
      return false;
    }

    // The Permissions API is reliable in browsers; the macOS WKWebView can report
    // "denied" spuriously, so on native we skip the gate and let getUserMedia — which
    // wry grants at the webview layer, then the OS prompts via TCC — be the truth.
    if (IS_WEB && "permissions" in navigator && typeof navigator.permissions?.query === "function") {
      try {
        const permission = await navigator.permissions.query({ name: "microphone" as PermissionName });
        if (permission.state === "denied") {
          onError("Microphone permission is blocked for CodeTerm.");
          return false;
        }
      } catch {
        // Permission probing can fail in older/sandboxed environments. Fall back to getUserMedia.
      }
    }

    try {
      const stream = await acquireWarmStream();
      const mime = pickAudioMime();
      mimeRef.current = mime;
      const rec = createRecorder(stream, mime);
      chunksRef.current = [];
      recDebugRef.current = { chunks: 0, bytes: 0, lastChunkBytes: 0, lastChunkAt: 0 };
      setLivePeaks(quietLiveWaveform());
      stopKindRef.current = "send";
      startedAtRef.current = performance.now();
      rec.ondataavailable = (e) => {
        recDebugRef.current = {
          chunks: recDebugRef.current.chunks + 1,
          bytes: recDebugRef.current.bytes + e.data.size,
          lastChunkBytes: e.data.size,
          lastChunkAt: performance.now(),
        };
        console.debug("[voice] recorder chunk", {
          bytes: e.data.size,
          type: e.data.type,
          state: rec.state,
          chunks: recDebugRef.current.chunks,
        });
        if (e.data.size > 0) chunksRef.current.push(e.data);
      };
      rec.onerror = (e) => console.error("[voice] recorder error:", e);
      rec.onstop = () => {
        const kind = stopKindRef.current;
        const durationSec = Math.max(0, (performance.now() - startedAtRef.current) / 1000);
        const peaks = buildWaveform(peaksRef.current);
        // Keep streamRef alive (warm) — only teardown on unmount stops the mic.
        teardownAnalyser();

        if (kind === "discard") {
          chunksRef.current = [];
          stopResolveRef.current?.([]);
          stopResolveRef.current = null;
          return;
        }

        const finalized = finalizeBlob();
        if (!finalized) {
          reportEmptyRecording(kind, durationSec);
          stopResolveRef.current?.(kind === "preview" ? false : []);
          stopResolveRef.current = null;
          return;
        }

        if (kind === "preview") {
          setPreview({
            url: URL.createObjectURL(finalized.blob),
            durationSec,
            peaks,
            blob: finalized.blob,
            mime: finalized.mime,
          });
          stopResolveRef.current?.(true);
          stopResolveRef.current = null;
          return;
        }

        void uploadBlob(finalized.blob, finalized.mime)
          .then((paths) => stopResolveRef.current?.(paths))
          .catch((e) => {
            onError(e instanceof Error ? e.message : String(e));
            stopResolveRef.current?.([]);
          })
          .finally(() => {
            stopResolveRef.current = null;
          });
      };
      rec.start(250);
      recRef.current = rec;
      startSampling(stream);
      setRecording(true);
      return true;
    } catch (e) {
      streamRef.current?.getTracks().forEach((t) => t.stop());
      streamRef.current = null;
      teardownAnalyser();
      setLivePeaks(quietLiveWaveform());
      onError(microphoneErrorMessage(e));
      return false;
    }
  }

  async function uploadBlob(blob: Blob, mime: string): Promise<string[]> {
    const path = await storeBlob(blob, `voice-note-${Date.now()}.${audioExt(mime)}`);
    return path.trim() ? [path] : [];
  }

  function stopWith(kind: StopKind): Promise<string[] | boolean> {
    const rec = recRef.current;
    if (!rec || rec.state === "inactive") {
      setRecording(false);
      return Promise.resolve(kind === "preview" ? false : []);
    }
    return new Promise((resolve) => {
      stopKindRef.current = kind;
      stopResolveRef.current = resolve;
      try {
        rec.requestData();
      } catch (e) {
        console.debug("[voice] recorder requestData before stop failed:", e);
      }
      rec.stop();
      recRef.current = null;
      setRecording(false);
      setLivePeaks(quietLiveWaveform());
    });
  }

  function stop(discard = false): Promise<string[]> {
    return stopWith(discard ? "discard" : "send") as Promise<string[]>;
  }

  /** Stop and hold the clip locally for review; resolves true if a usable clip was captured. */
  function stopForPreview(): Promise<boolean> {
    return stopWith("preview") as Promise<boolean>;
  }

  async function commitPreview(): Promise<string[]> {
    const p = preview;
    if (!p) return [];
    URL.revokeObjectURL(p.url);
    setPreview(null);
    try {
      return await uploadBlob(p.blob, p.mime);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
      return [];
    }
  }

  function discardPreview() {
    setPreview((p) => {
      if (p) URL.revokeObjectURL(p.url);
      return null;
    });
  }

  return { recording, elapsed, preview, livePeaks, start, stop, stopForPreview, commitPreview, discardPreview };
}
