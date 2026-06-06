/**
 * Composer affordances for every platform: attach-file, voice note,
 * explicit send (touch screens have no Enter habit).
 *
 * Browser files can't be referenced by path — storeBlob uploads them to
 * the daemon (POST /api/upload) in web mode or writes a temp file via the
 * Tauri command in desktop mode; either way the attached path is readable
 * by the agent CLI. Voice notes ride the same pipe; if the receiving pane
 * has no transcriber it will say so — the capability is the composer's
 * job, transcription is the pane's.
 */
import {
  useEffect,
  useRef,
  useState,
  type DragEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import { Mic, Paperclip, SendHorizontal } from "lucide-react";
import { invoke } from "../../lib/bridge";
import { IS_WEB, webUpload } from "../../lib/webApi";

function pickAudioMime(): string | undefined {
  if (typeof MediaRecorder === "undefined") return undefined;
  // iOS Safari records mp4/aac; everything else does webm/opus.
  return ["audio/webm;codecs=opus", "audio/webm", "audio/mp4"].find((m) =>
    MediaRecorder.isTypeSupported(m),
  );
}

function audioExt(mime: string): string {
  if (mime.includes("mp4")) return "m4a";
  if (mime.includes("ogg")) return "ogg";
  return "webm";
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onloadend = () => {
      const result = reader.result;
      if (typeof result !== "string") return reject(new Error("read failed"));
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

export function useVoiceRecorder(onError: (msg: string) => void) {
  const [recording, setRecording] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const recRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const streamRef = useRef<MediaStream | null>(null);
  const stopResolveRef = useRef<((paths: string[]) => void) | null>(null);

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
    },
    [],
  );

  async function start() {
    if (!navigator.mediaDevices?.getUserMedia) {
      // getUserMedia needs a secure context — plain-HTTP peers (e.g. a
      // raw Tailscale IP) can't record; `tailscale serve` adds the TLS.
      onError("Voice needs HTTPS (or localhost) — the mic API is unavailable here.");
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const mime = pickAudioMime();
      const rec = new MediaRecorder(stream, mime ? { mimeType: mime } : undefined);
      streamRef.current = stream;
      chunksRef.current = [];
      rec.ondataavailable = (e) => {
        if (e.data.size > 0) chunksRef.current.push(e.data);
      };
      rec.onstop = () => {
        stream.getTracks().forEach((t) => t.stop());
        streamRef.current = null;
        const type = rec.mimeType || mime || "audio/webm";
        const blob = new Blob(chunksRef.current, { type });
        if (blob.size === 0) {
          stopResolveRef.current?.([]);
          stopResolveRef.current = null;
          return;
        }
        void storeBlob(blob, `voice-note-${Date.now()}.${audioExt(type)}`)
          .then((path) => stopResolveRef.current?.([path]))
          .catch((e) => {
            onError(e instanceof Error ? e.message : String(e));
            stopResolveRef.current?.([]);
          })
          .finally(() => {
            stopResolveRef.current = null;
          });
      };
      rec.start();
      recRef.current = rec;
      setRecording(true);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  }

  function stop(): Promise<string[]> {
    const rec = recRef.current;
    if (!rec || rec.state === "inactive") {
      setRecording(false);
      return Promise.resolve([]);
    }
    return new Promise((resolve) => {
      stopResolveRef.current = resolve;
      rec.stop();
      recRef.current = null;
      setRecording(false);
    });
  }

  return { recording, elapsed, start, stop };
}

export async function storeBrowserFiles(
  files: Iterable<File>,
  onAttach: (paths: string[]) => void,
  onError: (msg: string) => void,
): Promise<void> {
  const paths: string[] = [];
  for (const f of files) {
    try {
      paths.push(await storeBlob(f, f.name || "file.bin"));
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  }
  if (paths.length > 0) onAttach(paths);
}

/** Browser drag-drop → upload → attach server paths. Web only: the Tauri
 * window gets native paths from useTauriFileDrop. */
export function useWebFileDrop(
  onAttach: (paths: string[]) => void,
  onError: (msg: string) => void,
) {
  const [dragOver, setDragOver] = useState(false);
  if (!IS_WEB) {
    return { dragOver: false, handlers: {} };
  }
  const handlers = {
    onDragOver: (e: DragEvent) => {
      if (e.dataTransfer.types.includes("Files")) {
        e.preventDefault();
        setDragOver(true);
      }
    },
    onDragLeave: (e: DragEvent) => {
      if (
        e.currentTarget === e.target ||
        !e.currentTarget.contains(e.relatedTarget as Node)
      ) {
        setDragOver(false);
      }
    },
    onDrop: (e: DragEvent) => {
      setDragOver(false);
      if (e.dataTransfer.files.length === 0) return;
      e.preventDefault();
      void storeBrowserFiles(Array.from(e.dataTransfer.files), onAttach, onError);
    },
  };
  return { dragOver, handlers };
}

interface Props {
  onAttach: (paths: string[]) => void;
  onSend: () => void;
  onSendWithAttachments: (paths: string[]) => void;
  onError: (msg: string) => void;
  sendDisabled: boolean;
  hasText: boolean;
  children: ReactNode;
}

const btnBase =
  "flex items-center justify-center w-9 h-9 shrink-0 rounded-md border transition-all duration-150";

export function ComposerActions({
  onAttach,
  onSend,
  onSendWithAttachments,
  onError,
  sendDisabled,
  hasText,
  children,
}: Props) {
  const fileRef = useRef<HTMLInputElement>(null);
  const [uploading, setUploading] = useState(false);
  const [voiceMode, setVoiceMode] = useState<"hold" | "locked" | null>(null);
  const [voiceBusy, setVoiceBusy] = useState(false);
  const voice = useVoiceRecorder(onError);

  async function finishVoice(sendAfter: boolean) {
    if (!voice.recording || voiceBusy) return;
    setVoiceBusy(true);
    const paths = await voice.stop();
    setVoiceMode(null);
    setVoiceBusy(false);
    if (paths.length === 0) return;
    if (sendAfter) {
      onSendWithAttachments(paths);
    } else {
      onAttach(paths);
    }
  }

  async function startVoice(e: PointerEvent<HTMLButtonElement>) {
    if (hasText) {
      onSend();
      return;
    }
    e.currentTarget.setPointerCapture(e.pointerId);
    setVoiceMode("hold");
    await voice.start();
  }

  function releaseVoice(e: PointerEvent<HTMLButtonElement>) {
    if (!voice.recording || voiceMode !== "hold") return;
    const rect = e.currentTarget.getBoundingClientRect();
    const inside =
      e.clientX >= rect.left &&
      e.clientX <= rect.right &&
      e.clientY >= rect.top &&
      e.clientY <= rect.bottom;
    if (inside) {
      void finishVoice(true);
    } else {
      setVoiceMode("locked");
    }
  }

  const mainIsSend = hasText || voiceMode === "locked";
  const mainTitle = mainIsSend
    ? voiceMode === "locked"
      ? "Stop, attach, and send voice note"
      : "Send"
    : "Hold to record voice note";

  return (
    <>
      <input
        ref={fileRef}
        type="file"
        multiple
        className="hidden"
        data-testid="composer-attach-input"
        onChange={(e) => {
          const files = e.target.files;
          if (!files || files.length === 0) return;
          setUploading(true);
          void storeBrowserFiles(Array.from(files), onAttach, onError).finally(() => {
            setUploading(false);
            if (fileRef.current) fileRef.current.value = "";
          });
        }}
      />
      <button
        type="button"
        data-testid="composer-attach-button"
        title="Attach file"
        disabled={uploading}
        onClick={() => fileRef.current?.click()}
        className={`${btnBase} border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] disabled:opacity-40`}
      >
        <Paperclip size={16} className={uploading ? "animate-pulse" : undefined} />
      </button>
      {children}
      <button
        type="button"
        data-testid={mainIsSend ? "composer-send-button" : "composer-voice-button"}
        title={mainTitle}
        onPointerDown={(e) => {
          if (mainIsSend) return;
          void startVoice(e);
        }}
        onPointerUp={(e) => {
          if (!mainIsSend) {
            releaseVoice(e);
            return;
          }
          if (voiceMode === "locked") {
            void finishVoice(true);
          } else {
            onSend();
          }
        }}
        onPointerCancel={() => {
          if (voice.recording && voiceMode === "hold") setVoiceMode("locked");
        }}
        onClick={(e) => e.preventDefault()}
        className={`${btnBase} ${
          voice.recording
            ? "border-[var(--ct-red,#f85149)] text-[var(--ct-red,#f85149)]"
            : mainIsSend
              ? "border-[var(--ct-accent,#58a6ff)] text-[var(--ct-accent,#58a6ff)] hover:bg-[var(--ct-hover)]"
              : "border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
        } ${voiceMode === "hold" ? "scale-[2.75] rounded-full shadow-2xl z-20" : ""} disabled:opacity-40`}
        style={
          voice.recording
            ? {
                background:
                  "color-mix(in srgb, var(--ct-red, #f85149) 12%, transparent)",
              }
            : undefined
        }
        disabled={voiceBusy || (hasText && sendDisabled)}
      >
        {mainIsSend ? <SendHorizontal size={16} /> : <Mic size={16} />}
      </button>
      {voice.recording && (
        <span
          data-testid="composer-voice-elapsed"
          className="text-[11px] font-mono text-[var(--ct-red,#f85149)] shrink-0"
        >
          {Math.floor(voice.elapsed / 60)}:{String(voice.elapsed % 60).padStart(2, "0")}
        </span>
      )}
    </>
  );
}
