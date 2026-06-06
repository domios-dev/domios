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
import { Mic, Paperclip, SendHorizontal, Terminal } from "lucide-react";
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

function microphoneErrorMessage(error: unknown): string {
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
  const discardRef = useRef(false);

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

  async function start(): Promise<boolean> {
    if (!navigator.mediaDevices?.getUserMedia) {
      onError(
        IS_WEB
          ? "Voice needs HTTPS on phones. Open Mesh -> Web UI and scan/copy the https://*.ts.net link; http://100.x cannot use the mic."
          : "Microphone capture is unavailable in this CodeTerm window. Restart the app after updating microphone permissions.",
      );
      return false;
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
        if (discardRef.current) {
          discardRef.current = false;
          chunksRef.current = [];
          stopResolveRef.current?.([]);
          stopResolveRef.current = null;
          return;
        }
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
      return true;
    } catch (e) {
      onError(microphoneErrorMessage(e));
      return false;
    }
  }

  function stop(discard = false): Promise<string[]> {
    const rec = recRef.current;
    if (!rec || rec.state === "inactive") {
      setRecording(false);
      return Promise.resolve([]);
    }
    return new Promise((resolve) => {
      discardRef.current = discard;
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
  onToggleTerminalFallback?: () => void;
  children: ReactNode;
}

const btnBase =
  "flex items-center justify-center shrink-0 transition-all duration-150";

export function ComposerActions({
  onAttach,
  onSend,
  onSendWithAttachments,
  onError,
  sendDisabled,
  hasText,
  onToggleTerminalFallback,
  children,
}: Props) {
  const fileRef = useRef<HTMLInputElement>(null);
  const [uploading, setUploading] = useState(false);
  const [voiceMode, setVoiceMode] = useState<"hold" | "locked" | null>(null);
  const [voiceBusy, setVoiceBusy] = useState(false);
  const voiceStartXRef = useRef<number | null>(null);
  const voiceStartYRef = useRef<number | null>(null);
  const voiceLockedFromDragRef = useRef(false);
  const voice = useVoiceRecorder(onError);

  async function finishVoice(sendAfter: boolean) {
    if (voiceBusy) return;
    setVoiceBusy(true);
    const paths = await voice.stop();
    setVoiceMode(null);
    voiceStartXRef.current = null;
    voiceStartYRef.current = null;
    voiceLockedFromDragRef.current = false;
    setVoiceBusy(false);
    if (paths.length === 0) return;
    if (sendAfter) {
      onSendWithAttachments(paths);
    } else {
      onAttach(paths);
    }
  }

  async function cancelVoice() {
    if (voiceBusy) return;
    setVoiceBusy(true);
    await voice.stop(true);
    setVoiceMode(null);
    voiceStartXRef.current = null;
    voiceStartYRef.current = null;
    voiceLockedFromDragRef.current = false;
    setVoiceBusy(false);
  }

  async function startVoice(e: PointerEvent<HTMLButtonElement>) {
    if (hasText) {
      onSend();
      return;
    }
    e.currentTarget.setPointerCapture(e.pointerId);
    voiceStartXRef.current = e.clientX;
    voiceStartYRef.current = e.clientY;
    voiceLockedFromDragRef.current = false;
    const started = await voice.start();
    if (started) {
      setVoiceMode("hold");
      if (e.buttons !== 1) {
        const paths = await voice.stop();
        setVoiceMode(null);
        voiceStartXRef.current = null;
        voiceStartYRef.current = null;
        voiceLockedFromDragRef.current = false;
        if (paths.length > 0) onSendWithAttachments(paths);
      }
    } else {
      voiceStartXRef.current = null;
      voiceStartYRef.current = null;
      voiceLockedFromDragRef.current = false;
      try {
        e.currentTarget.releasePointerCapture(e.pointerId);
      } catch {
        /* already released */
      }
    }
  }

  function releaseVoice(e: PointerEvent<HTMLButtonElement>) {
    if (!voice.recording) return;
    if (voiceMode === "locked") {
      if (voiceLockedFromDragRef.current) {
        voiceLockedFromDragRef.current = false;
        try {
          e.currentTarget.releasePointerCapture(e.pointerId);
        } catch {
          /* already released */
        }
        return;
      }
      void finishVoice(true);
      return;
    }
    if (voiceMode !== "hold") return;
    if (voiceLockedFromDragRef.current) {
      voiceLockedFromDragRef.current = false;
      setVoiceMode("locked");
      try {
        e.currentTarget.releasePointerCapture(e.pointerId);
      } catch {
        /* already released */
      }
      return;
    }
    const rect = e.currentTarget.getBoundingClientRect();
    const inside =
      e.clientX >= rect.left &&
      e.clientX <= rect.right &&
      e.clientY >= rect.top &&
      e.clientY <= rect.bottom;
    if (inside) {
      void finishVoice(true);
    } else {
      voiceLockedFromDragRef.current = true;
      setVoiceMode("locked");
    }
  }

  function moveVoice(e: PointerEvent<HTMLButtonElement>) {
    if (!voice.recording || voiceMode !== "hold" || voiceStartXRef.current == null || voiceStartYRef.current == null) return;
    const dx = e.clientX - voiceStartXRef.current;
    const dy = e.clientY - voiceStartYRef.current;
    if (dx <= -95) {
      void cancelVoice();
      return;
    }
    if (Math.abs(dx) > 28 || Math.abs(dy) > 28) {
      voiceLockedFromDragRef.current = true;
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
      <div className="flex w-full items-end gap-2">
        <button
          type="button"
          data-testid="composer-attach-button"
          title="Attach file"
          disabled={uploading}
          onClick={() => fileRef.current?.click()}
          className={`${btnBase} h-11 w-11 rounded-full border border-white/10 text-white/90 shadow-[0_2px_10px_rgba(0,0,0,0.28)] hover:scale-[1.03] disabled:opacity-40`}
          style={{
            background:
              "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 82%, white 8%)",
          }}
        >
          <Paperclip size={22} strokeWidth={2.3} className={uploading ? "animate-pulse" : undefined} />
        </button>
        <div
          className="relative flex min-h-11 flex-1 items-end rounded-[1.45rem] border px-4 py-2 pr-12 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]"
          style={{
            background:
              "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 86%, white 7%)",
            borderColor:
              "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
          }}
        >
          {voice.recording ? (
            <div
              data-testid="composer-voice-cancel-hint"
              role="button"
              tabIndex={0}
              onPointerDown={(e) => {
                e.preventDefault();
                void cancelVoice();
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  void cancelVoice();
                }
              }}
              className="absolute inset-1 z-10 flex items-center justify-center rounded-[1.2rem] text-[14px] font-medium text-[var(--ct-fg)]"
              style={{
                background:
                  "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 92%, var(--ct-red, #f85149) 10%)",
                border: "1px solid color-mix(in srgb, var(--ct-red, #f85149) 35%, transparent)",
              }}
            >
              Cancel recording
            </div>
          ) : null}
          <div className={voice.recording ? "w-full opacity-0 pointer-events-none" : "w-full"}>
            {children}
          </div>
          {onToggleTerminalFallback && (
            <button
              type="button"
              data-testid="composer-terminal-toggle"
              data-terminal-fallback-toggle="true"
              title="Toggle terminal fallback"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => onToggleTerminalFallback()}
              className="absolute bottom-2 right-2 flex h-7 w-7 items-center justify-center rounded-full text-[var(--ct-muted-fg)] transition-colors hover:bg-white/10 hover:text-[var(--ct-fg)]"
              style={{ background: "rgba(255,255,255,0.035)" }}
            >
              <Terminal size={15} />
            </button>
          )}
        </div>
        <button
          type="button"
          data-testid={mainIsSend ? "composer-send-button" : "composer-voice-button"}
          title={mainTitle}
          onPointerDown={(e) => {
            if (mainIsSend) return;
            void startVoice(e);
          }}
          onPointerUp={(e) => {
            if (voice.recording) {
              releaseVoice(e);
              return;
            }
            if (mainIsSend) onSend();
          }}
          onPointerCancel={() => {
            if (voice.recording && voiceMode === "hold") {
              voiceLockedFromDragRef.current = true;
              setVoiceMode("locked");
            }
          }}
          onPointerMove={moveVoice}
          onClick={(e) => e.preventDefault()}
          className={`${btnBase} h-12 w-12 rounded-full border text-white shadow-[0_5px_18px_rgba(31,111,235,0.35)] hover:scale-[1.03] active:scale-95 ${
            voice.recording ? "z-20 scale-[2.35] shadow-[0_18px_55px_rgba(31,111,235,0.52)]" : ""
          } ${voiceMode === "locked" ? "ring-4 ring-[var(--ct-accent,#58a6ff)]/20" : ""} disabled:opacity-40`}
          style={{
            background:
              voice.recording || mainIsSend
                ? "linear-gradient(145deg, #4f8cff 0%, #1f6feb 100%)"
                : "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 78%, white 9%)",
            borderColor:
              voice.recording || mainIsSend
                ? "rgba(255,255,255,0.18)"
                : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 24%, transparent)",
            color: voice.recording || mainIsSend ? "white" : "var(--ct-fg, #c9d1d9)",
          }}
          disabled={voiceBusy || (hasText && sendDisabled)}
        >
          {mainIsSend ? <SendHorizontal size={23} fill="currentColor" /> : <Mic size={22} />}
        </button>
      </div>
      {voice.recording && (
        <span
          data-testid="composer-voice-elapsed"
          className="absolute -top-6 right-0 rounded-full bg-[var(--ct-titlebar-bg)] px-2 py-0.5 text-[11px] font-mono text-[var(--ct-accent,#58a6ff)] shadow"
        >
          {Math.floor(voice.elapsed / 60)}:{String(voice.elapsed % 60).padStart(2, "0")}
        </span>
      )}
    </>
  );
}
