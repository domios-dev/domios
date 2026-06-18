import { invoke } from "../../../lib/bridge";
import { IS_WEB, webUpload } from "../../../lib/webApi";

// A near-instant tap (e.g. finger lifted while granting the mic permission) yields a container-header-only clip
// — non-zero but a few KB with no real audio. Discard anything under this floor so empty voice notes never send.
export const MIN_VOICE_BYTES = 5 * 1024;
export const TEST_AUDIO_MIME = "audio/webm";

type FrontendTestEnv = {
  DEV?: boolean;
  VITE_CODETERM_TEST?: string;
};

type TestAudioState = {
  getUserMediaCalls: number;
  recorderStarts: number;
  chunks: number;
};

declare global {
  interface Window {
    __codetermTestAudio?: TestAudioState;
    __gumCalls?: number;
  }
}

function importEnv(): FrontendTestEnv | undefined {
  return (import.meta as { env?: FrontendTestEnv }).env;
}

export function isCodeTermFrontendTestMode(env: FrontendTestEnv | undefined = importEnv()): boolean {
  return env?.DEV === true && env.VITE_CODETERM_TEST === "1";
}

function makeTestTrack(): MediaStreamTrack {
  let readyState: MediaStreamTrackState = "live";
  return {
    id: "codeterm-test-audio-track",
    kind: "audio",
    label: "CodeTerm Test Microphone",
    enabled: true,
    muted: false,
    get readyState() {
      return readyState;
    },
    stop() {
      readyState = "ended";
    },
    getSettings() {
      return { deviceId: "codeterm-test-mic" } as MediaTrackSettings;
    },
  } as MediaStreamTrack;
}

function makeTestStream(): MediaStream {
  const track = makeTestTrack();
  return {
    get active() {
      return track.readyState === "live";
    },
    id: "codeterm-test-audio-stream",
    getAudioTracks: () => [track],
    getTracks: () => [track],
  } as MediaStream;
}

function makeTestAudioBlob(): Blob {
  const bytes = new Uint8Array(MIN_VOICE_BYTES + 1024);
  for (let i = 0; i < bytes.length; i++) bytes[i] = (i * 31) % 251;
  return new Blob([bytes], { type: TEST_AUDIO_MIME });
}

function emitRecorderEvent(
  recorder: EventTarget & { ondataavailable: ((event: BlobEvent) => void) | null },
): void {
  const blob = makeTestAudioBlob();
  window.__codetermTestAudio!.chunks += 1;
  recorder.ondataavailable?.({ data: blob } as BlobEvent);
}

function createTestMediaRecorder(): typeof MediaRecorder {
  class CodeTermTestMediaRecorder extends EventTarget {
    static isTypeSupported(mime: string): boolean {
      return mime === TEST_AUDIO_MIME || mime.startsWith("audio/");
    }

    readonly stream: MediaStream;
    readonly mimeType: string;
    state: RecordingState = "inactive";
    ondataavailable: ((event: BlobEvent) => void) | null = null;
    onstop: ((event: Event) => void) | null = null;
    onerror: ((event: Event) => void) | null = null;
    private timer: number | null = null;

    constructor(stream: MediaStream, options?: MediaRecorderOptions) {
      super();
      this.stream = stream;
      this.mimeType = options?.mimeType || TEST_AUDIO_MIME;
    }

    start(timeslice?: number): void {
      if (this.state !== "inactive") {
        throw new DOMException("Recorder is already active", "InvalidStateError");
      }
      window.__codetermTestAudio!.recorderStarts += 1;
      this.state = "recording";
      if (timeslice && timeslice > 0) {
        this.timer = window.setInterval(() => {
          if (this.state === "recording") emitRecorderEvent(this);
        }, Math.max(1, timeslice));
      }
    }

    stop(): void {
      if (this.state === "inactive") return;
      if (this.timer != null) {
        window.clearInterval(this.timer);
        this.timer = null;
      }
      emitRecorderEvent(this);
      this.state = "inactive";
      queueMicrotask(() => this.onstop?.(new Event("stop")));
    }

    pause(): void {
      if (this.state === "recording") this.state = "paused";
    }

    resume(): void {
      if (this.state === "paused") this.state = "recording";
    }

    requestData(): void {
      if (this.state !== "inactive") emitRecorderEvent(this);
    }
  }

  return CodeTermTestMediaRecorder as unknown as typeof MediaRecorder;
}

export function installCodeTermTestAudioMocks(testMode = isCodeTermFrontendTestMode()): void {
  if (!testMode || typeof window === "undefined" || typeof navigator === "undefined") return;
  window.__codetermTestAudio ??= { getUserMediaCalls: 0, recorderStarts: 0, chunks: 0 };

  const mediaDevices = {
    ...(navigator.mediaDevices ?? {}),
    getUserMedia: async () => {
      window.__codetermTestAudio!.getUserMediaCalls += 1;
      if (typeof window.__gumCalls === "number") window.__gumCalls += 1;
      return makeTestStream();
    },
  } as MediaDevices;

  try {
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: mediaDevices,
    });
  } catch {
    (navigator as unknown as { mediaDevices: MediaDevices }).mediaDevices = mediaDevices;
  }

  try {
    Object.defineProperty(window, "MediaRecorder", {
      configurable: true,
      value: createTestMediaRecorder(),
    });
  } catch {
    window.MediaRecorder = createTestMediaRecorder();
  }
}

installCodeTermTestAudioMocks();

export function pickAudioMime(): string | undefined {
  if (isCodeTermFrontendTestMode() || (typeof window !== "undefined" && window.__codetermTestAudio)) {
    return TEST_AUDIO_MIME;
  }
  if (typeof MediaRecorder === "undefined") return undefined;
  const appleNative =
    !IS_WEB &&
    /\b(Macintosh|Mac OS X|iPhone|iPad|iPod)\b/i.test(navigator.userAgent);
  const candidates = appleNative
    ? ["audio/mp4", "audio/aac", "audio/webm;codecs=opus", "audio/webm", "audio/ogg;codecs=opus", "audio/ogg"]
    : ["audio/ogg;codecs=opus", "audio/ogg", "audio/webm;codecs=opus", "audio/webm", "audio/mp4", "audio/aac"];
  return candidates.find((m) => MediaRecorder.isTypeSupported(m));
}

export function audioExt(mime: string): string {
  if (mime.includes("ogg")) return "ogg";
  if (mime.includes("mp4")) return "m4a";
  return "webm";
}

export function microphoneErrorMessage(error: unknown): string {
  if (error instanceof DOMException) {
    if (error.name === "NotAllowedError" || error.name === "SecurityError") {
      return "Microphone permission is blocked for CodeTerm.";
    }
    if (error.name === "NotFoundError") {
      return "No microphone was found for CodeTerm.";
    }
  }
  return error instanceof Error ? error.message : String(error);
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.onerror = () => reject(reader.error ?? new Error("read failed"));
    reader.readAsDataURL(blob);
  });
}

/** Persist a browser blob where the agent can read it: daemon upload in
 * web mode, local temp file via Tauri in desktop mode. */
export async function storeBlob(blob: Blob, filename: string): Promise<string> {
  if (IS_WEB) {
    return (await webUpload(blob, filename)).path;
  }
  const ext = filename.includes(".") ? filename.split(".").pop()! : "bin";
  return invoke<string>("save_temp_file", {
    data: await blobToBase64(blob),
    extension: ext,
    name: filename,
  });
}
