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
  return ["audio/ogg;codecs=opus", "audio/ogg", "audio/webm;codecs=opus", "audio/webm", "audio/mp4"].find((m) =>
    MediaRecorder.isTypeSupported(m),
  );
}

function audioExt(mime: string): string {
  if (mime.includes("ogg")) return "ogg";
  if (mime.includes("mp4")) return "m4a";
  return "webm";
}

function pointInRect(x: number, y: number, rect: DOMRect | undefined): boolean {
  return rect != null && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
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

    if ("permissions" in navigator && typeof navigator.permissions?.query === "function") {
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
          .then((path) => {
            if (path.trim()) {
              stopResolveRef.current?.([path]);
            } else {
              stopResolveRef.current?.([]);
            }
          })
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
      streamRef.current?.getTracks().forEach((t) => t.stop());
      streamRef.current = null;
      onError(microphoneErrorMessage(e));
      return false;
    }
  }

  function stop(discard = false): Promise<string[]> {
    const rec = recRef.current;
    if (!rec || rec.state === "inactive") {
      setRecording(false);
      discardRef.current = false;
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
      const path = await storeBlob(f, f.name || "file.bin");
      if (path.trim()) {
        paths.push(path);
      }
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
  "flex items-center justify-center shrink-0 transition-all duration-150";

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
  const buttonRef = useRef<HTMLButtonElement>(null);
  const cancelHintRef = useRef<HTMLDivElement>(null);
  const [uploading, setUploading] = useState(false);
  const [voiceMode, setVoiceMode] = useState<"hold" | "locked" | null>(null);
  const [voicePressed, setVoicePressed] = useState(false);
  const [voiceBusy, setVoiceBusy] = useState(false);
  const voiceStartXRef = useRef<number | null>(null);
  const voiceStartYRef = useRef<number | null>(null);
  const voicePointerXRef = useRef<number | null>(null);
  const voicePointerYRef = useRef<number | null>(null);
  const voiceLockedFromDragRef = useRef(false);
  const activePointerIdRef = useRef<number | null>(null);
  const voiceModeRef = useRef<typeof voiceMode>(voiceMode);
  const voiceBusyRef = useRef(voiceBusy);
  const voice = useVoiceRecorder(onError);
  const voiceRecordingRef = useRef(voice.recording);

  useEffect(() => {
    voiceModeRef.current = voiceMode;
    voiceBusyRef.current = voiceBusy;
    voiceRecordingRef.current = voice.recording;
  }, [voiceMode, voiceBusy, voice.recording]);

  function resetVoicePointer() {
    activePointerIdRef.current = null;
    voiceStartXRef.current = null;
    voiceStartYRef.current = null;
    voicePointerXRef.current = null;
    voicePointerYRef.current = null;
    voiceLockedFromDragRef.current = false;
  }

  async function finishVoice(sendAfter: boolean) {
    if (voiceBusy) return;
    setVoiceBusy(true);
    const paths = await voice.stop();
    setVoiceMode(null);
    setVoicePressed(false);
    resetVoicePointer();
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
    setVoicePressed(false);
    resetVoicePointer();
    setVoiceBusy(false);
  }

  async function startVoice(e: PointerEvent<HTMLButtonElement>) {
    if (hasText) {
      onSend();
      return;
    }
    setVoicePressed(true);
    activePointerIdRef.current = e.pointerId;
    e.currentTarget.setPointerCapture(e.pointerId);
    voiceStartXRef.current = e.clientX;
    voiceStartYRef.current = e.clientY;
    voicePointerXRef.current = e.clientX;
    voicePointerYRef.current = e.clientY;
    voiceLockedFromDragRef.current = false;
    const started = await voice.start();
    if (started) {
      if (activePointerIdRef.current !== e.pointerId) {
        const paths = await voice.stop();
        setVoiceMode(null);
        setVoicePressed(false);
        resetVoicePointer();
        if (paths.length > 0) onSendWithAttachments(paths);
        return;
      }
      setVoiceMode("hold");
      const pointerX = voicePointerXRef.current;
      const pointerY = voicePointerYRef.current;
      if (
        pointerX != null &&
        pointerY != null &&
        !pointInRect(pointerX, pointerY, e.currentTarget.getBoundingClientRect())
      ) {
        voiceLockedFromDragRef.current = true;
        setVoiceMode("locked");
        return;
      }
      if (e.buttons !== 1) {
        const paths = await voice.stop();
        setVoiceMode(null);
        setVoicePressed(false);
        resetVoicePointer();
        if (paths.length > 0) onSendWithAttachments(paths);
      }
    } else {
      setVoicePressed(false);
      resetVoicePointer();
      try {
        e.currentTarget.releasePointerCapture(e.pointerId);
      } catch {
        /* already released */
      }
    }
  }

  function moveVoice(e: PointerEvent<HTMLButtonElement>) {
    if (!voice.recording || voiceStartXRef.current == null || voiceStartYRef.current == null) return;
    voicePointerXRef.current = e.clientX;
    voicePointerYRef.current = e.clientY;
    if (voiceMode !== "hold") return;
    if (!pointInRect(e.clientX, e.clientY, e.currentTarget.getBoundingClientRect())) {
      voiceLockedFromDragRef.current = true;
      setVoiceMode("locked");
    }
  }

  useEffect(() => {
    function onWindowPointerMove(e: globalThis.PointerEvent) {
      if (activePointerIdRef.current !== e.pointerId) return;
      voicePointerXRef.current = e.clientX;
      voicePointerYRef.current = e.clientY;
      if (!voiceRecordingRef.current) return;
      if (voiceModeRef.current !== "hold") return;
      const buttonRect = buttonRef.current?.getBoundingClientRect();
      if (!pointInRect(e.clientX, e.clientY, buttonRect)) {
        voiceLockedFromDragRef.current = true;
        setVoiceMode("locked");
      }
    }

    function onWindowPointerUp(e: globalThis.PointerEvent) {
      if (activePointerIdRef.current !== e.pointerId) return;
      const button = buttonRef.current;
      if (button?.hasPointerCapture(e.pointerId)) {
        try {
          button.releasePointerCapture(e.pointerId);
        } catch {
          /* already released */
        }
      }
      if (!voiceRecordingRef.current) {
        setVoicePressed(false);
        resetVoicePointer();
        return;
      }
      if (pointInRect(e.clientX, e.clientY, cancelHintRef.current?.getBoundingClientRect())) {
        void cancelVoice();
        return;
      }
      if (voiceModeRef.current === "locked") {
        void finishVoice(true);
        return;
      }
      if (pointInRect(e.clientX, e.clientY, button?.getBoundingClientRect())) {
        void finishVoice(true);
      } else {
        setVoiceMode("locked");
      }
    }

    function onWindowPointerCancel(e: globalThis.PointerEvent) {
      if (activePointerIdRef.current !== e.pointerId) return;
      if (voiceRecordingRef.current) {
        setVoiceMode("locked");
      } else {
        setVoicePressed(false);
        resetVoicePointer();
      }
    }

    window.addEventListener("pointermove", onWindowPointerMove);
    window.addEventListener("pointerup", onWindowPointerUp);
    window.addEventListener("pointercancel", onWindowPointerCancel);
    return () => {
      window.removeEventListener("pointermove", onWindowPointerMove);
      window.removeEventListener("pointerup", onWindowPointerUp);
      window.removeEventListener("pointercancel", onWindowPointerCancel);
    };
  }, []);

  const mainIsSend = hasText || voiceMode === "locked";
  const voiceActive = voicePressed || voice.recording;
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
      <div className="relative flex w-full items-end gap-1.5">
        <button
          type="button"
          data-testid="composer-attach-button"
          title="Attach file"
          disabled={uploading}
          onClick={() => fileRef.current?.click()}
          className={`${btnBase} ct-composer-round-button mb-1 h-10 w-10 rounded-full border border-white/10 text-white/90 shadow-[0_2px_10px_rgba(0,0,0,0.28)] hover:scale-[1.03] disabled:opacity-40`}
          style={{
            background:
              "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 82%, white 8%)",
          }}
        >
          <Paperclip size={20} strokeWidth={2.3} className={uploading ? "animate-pulse" : undefined} />
        </button>
        <div
          className="ct-composer-input-shell relative flex min-h-12 flex-1 items-center rounded-[1.35rem] border px-3.5 py-2 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]"
          style={{
            background:
              "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 86%, white 7%)",
            borderColor:
              "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
          }}
        >
          {voice.recording ? (
            <div
              ref={cancelHintRef}
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
              className="absolute inset-1 z-10 flex min-h-10 items-center justify-center rounded-[1.1rem] text-[13px] font-medium text-[var(--ct-fg)]"
              style={{
                background:
                  "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 92%, var(--ct-red, #f85149) 10%)",
                border: "1px solid color-mix(in srgb, var(--ct-red, #f85149) 35%, transparent)",
              }}
            >
              Cancel recording
            </div>
          ) : null}
          <div className={voice.recording ? "relative flex w-full items-center opacity-0 pointer-events-none" : "relative flex w-full items-center"}>
            {children}
          </div>
        </div>
        {/* mb-1 optically centers the 40px buttons against the 48px single-line input (items-end row). */}
        <div className="relative mb-1 h-10 w-10 shrink-0">
          <button
            ref={buttonRef}
            type="button"
            data-testid={mainIsSend ? "composer-send-button" : "composer-voice-button"}
            title={mainTitle}
            onPointerDown={(e) => {
              if (mainIsSend) {
                e.preventDefault();
                return;
              }
              void startVoice(e);
            }}
            onPointerUp={() => {
              if (voice.recording || voicePressed) {
                return;
              }
              if (mainIsSend) onSend();
            }}
            onTouchStart={(e) => {
              if (mainIsSend) e.preventDefault();
            }}
            onPointerCancel={() => {
              if (voice.recording && voiceMode === "hold") {
                voiceLockedFromDragRef.current = true;
                setVoiceMode("locked");
              }
            }}
            onPointerMove={moveVoice}
            // iOS steals composer focus via the synthesized mousedown after touchend — suppress it so the keyboard survives a send tap.
            onTouchEnd={(e) => {
              if (mainIsSend) e.preventDefault();
            }}
            onClick={(e) => e.preventDefault()}
            className={`${btnBase} ${voiceActive ? "" : "ct-composer-round-button"} absolute left-1/2 top-1/2 rounded-full border text-white ${
              voiceActive
                ? "z-30 shadow-[0_18px_55px_rgba(31,111,235,0.52)]"
                : "h-10 w-10"
            } ${voiceMode === "locked" ? "ring-4 ring-[var(--ct-accent,#58a6ff)]/20" : ""} ${
              voiceActive || mainIsSend
                ? "shadow-[0_5px_18px_rgba(31,111,235,0.35)]"
                : "shadow-[0_2px_10px_rgba(0,0,0,0.28)]"
            } disabled:opacity-40`}
            style={{
              background:
                voiceActive || mainIsSend
                  ? "linear-gradient(145deg, #4f8cff 0%, #1f6feb 100%)"
                  : "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 78%, white 9%)",
              borderColor:
                voiceActive || mainIsSend
                  ? "rgba(255,255,255,0.18)"
                  : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 24%, transparent)",
              color: voiceActive || mainIsSend ? "white" : "var(--ct-fg, #c9d1d9)",
              height: voiceActive ? "5.5rem" : undefined,
              width: voiceActive ? "5.5rem" : undefined,
              // Constant center anchor in both states — only the size transitions, so growth radiates from the center.
              transform: "translate(-50%, -50%)",
              touchAction: "none",
            }}
            disabled={voiceBusy || (hasText && sendDisabled)}
          >
            {mainIsSend ? (
              <SendHorizontal size={voiceActive ? 34 : 20} fill="currentColor" />
            ) : (
              <Mic size={voiceActive ? 32 : 20} />
            )}
          </button>
        </div>
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
