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
 *
 * Voice capture mirrors Telegram: hold the mic to record, slide left to
 * cancel, slide up to lock hands-free, then stop into a listen-before-send
 * preview. State machine + waveform live in ./voice/.
 */
import {
  useEffect,
  useRef,
  useState,
  type DragEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import { ChevronUp, Clock3, Lock, Mic, Paperclip, SendHorizontal, Square } from "lucide-react";
import { IS_WEB } from "../../lib/webApi";
import { storeBlob } from "./voice/audio";
import { useVoiceRecorder } from "./voice/useVoiceRecorder";
import { VoicePreview } from "./voice/VoicePreview";
import { VoiceRecordingBar } from "./voice/VoiceRecordingBar";

function pointInRect(x: number, y: number, rect: DOMRect | undefined): boolean {
  return rect != null && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
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
      if (path.trim()) paths.push(path);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  }
  if (paths.length > 0) onAttach(paths);
}

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
  onSchedule?: () => void;
  onSendWithAttachments: (paths: string[]) => void;
  onError: (msg: string) => void;
  sendDisabled: boolean;
  hasText: boolean;
  children: ReactNode;
}

const btnBase =
  "flex items-center justify-center shrink-0 transition-all duration-150";

type VoiceMode = "hold" | "locked" | "preview" | null;

export function ComposerActions({
  onAttach,
  onSend,
  onSchedule,
  onSendWithAttachments,
  onError,
  sendDisabled,
  hasText,
  children,
}: Props) {
  const fileRef = useRef<HTMLInputElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const shellRef = useRef<HTMLDivElement>(null);
  const cancelHintRef = useRef<HTMLDivElement>(null);
  const [uploading, setUploading] = useState(false);
  const [voiceMode, setVoiceMode] = useState<VoiceMode>(null);
  const [voicePressed, setVoicePressed] = useState(false);
  const [voiceBusy, setVoiceBusy] = useState(false);
  // Slide-left toward the input arms cancel; releasing while armed (or over the input) discards.
  const [cancelArmed, setCancelArmed] = useState(false);
  const cancelArmedRef = useRef(false);
  const voiceStartXRef = useRef<number | null>(null);
  const voiceStartYRef = useRef<number | null>(null);
  const voicePointerXRef = useRef<number | null>(null);
  const voicePointerYRef = useRef<number | null>(null);
  const voiceLockedFromDragRef = useRef(false);
  const activePointerIdRef = useRef<number | null>(null);
  const voiceModeRef = useRef<VoiceMode>(voiceMode);
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
    armCancel(false);
  }

  function armCancel(on: boolean) {
    cancelArmedRef.current = on;
    setCancelArmed(on);
  }

  // Releasing over the input (or its cancel hint) while holding discards the take.
  function overCancelZone(x: number, y: number): boolean {
    return (
      pointInRect(x, y, shellRef.current?.getBoundingClientRect()) ||
      pointInRect(x, y, cancelHintRef.current?.getBoundingClientRect())
    );
  }

  // Directional resolve while holding: drag up past the threshold locks hands-free; drag left toward the input arms cancel.
  function evalVoiceDrag(x: number, y: number) {
    if (voiceModeRef.current !== "hold") return;
    const sx = voiceStartXRef.current;
    const sy = voiceStartYRef.current;
    if (sx == null || sy == null) return;
    const up = sy - y;
    const left = sx - x;
    if (up > 48 && up >= left) {
      armCancel(false);
      voiceLockedFromDragRef.current = true;
      setVoiceMode("locked");
      return;
    }
    armCancel(left > 36 || overCancelZone(x, y));
  }

  // Quick hold-release: stop, upload, and send straight away.
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

  // Locked mode: stop into the listen-before-send preview instead of sending.
  async function stopToPreview() {
    if (voiceBusy) return;
    setVoiceBusy(true);
    const hasClip = await voice.stopForPreview();
    setVoicePressed(false);
    resetVoicePointer();
    setVoiceBusy(false);
    setVoiceMode(hasClip ? "preview" : null);
  }

  async function sendPreview() {
    if (voiceBusy) return;
    setVoiceBusy(true);
    const paths = await voice.commitPreview();
    setVoiceBusy(false);
    setVoiceMode(null);
    if (paths.length > 0) onSendWithAttachments(paths);
  }

  function discardPreviewClip() {
    voice.discardPreview();
    setVoiceMode(null);
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
        // Pointer already lifted (e.g. during the permission grant) — treat as a quick tap.
        await finishVoice(true);
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
        await finishVoice(true);
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
    evalVoiceDrag(e.clientX, e.clientY);
  }

  useEffect(() => {
    function onWindowPointerMove(e: globalThis.PointerEvent) {
      if (activePointerIdRef.current !== e.pointerId) return;
      voicePointerXRef.current = e.clientX;
      voicePointerYRef.current = e.clientY;
      if (!voiceRecordingRef.current) return;
      evalVoiceDrag(e.clientX, e.clientY);
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
      // Already locked → lifting the finger keeps recording hands-free.
      if (voiceModeRef.current === "locked") {
        setVoicePressed(false);
        return;
      }
      // Slid toward the input (armed) or released over it → discard.
      if (cancelArmedRef.current || overCancelZone(e.clientX, e.clientY)) {
        void cancelVoice();
        return;
      }
      // Released on the button or anywhere else with the finger up → send the take.
      void finishVoice(true);
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

  const isHold = voiceMode === "hold";
  const isLocked = voiceMode === "locked";
  const isPreview = voiceMode === "preview";
  const mainIsSend = hasText;
  const voiceActive = voicePressed || (voice.recording && !isLocked);
  const composerHidden = voice.recording || isPreview;
  const mainTitle = mainIsSend
    ? "Send"
    : isLocked
      ? "Stop and review voice note"
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
        {!isPreview && (
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
        )}
        <div
          ref={shellRef}
          className="ct-composer-input-shell relative flex min-h-12 flex-1 items-center rounded-[1.35rem] border px-3.5 py-2 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]"
          style={{
            background:
              "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 86%, white 7%)",
            borderColor:
              "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
          }}
        >
          {composerHidden && (
            <div className="absolute inset-1 z-10 flex items-center px-1.5">
              {isPreview && voice.preview ? (
                <VoicePreview
                  data={voice.preview}
                  busy={voiceBusy}
                  onDiscard={discardPreviewClip}
                  onSend={() => void sendPreview()}
                />
              ) : (
                <VoiceRecordingBar
                  mode={isLocked ? "locked" : "hold"}
                  armed={cancelArmed}
                  peaks={voice.livePeaks}
                  cancelHintRef={cancelHintRef}
                  onCancel={() => void cancelVoice()}
                />
              )}
            </div>
          )}
          <div
            className={
              composerHidden
                ? "relative flex min-w-0 flex-1 items-center opacity-0 pointer-events-none"
                : "relative flex min-w-0 flex-1 items-center"
            }
          >
            {children}
          </div>
          {!composerHidden && mainIsSend && onSchedule && (
            <button
              type="button"
              data-testid="composer-schedule-button"
              title="Schedule send"
              onMouseDown={(e) => e.preventDefault()}
              onClick={(e) => {
                e.preventDefault();
                onSchedule();
              }}
              className={`${btnBase} ml-1 h-8 w-8 shrink-0 rounded-full text-[var(--ct-muted-fg)] transition hover:text-[var(--ct-fg)] hover:brightness-125`}
              style={{
                background:
                  "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 82%, white 7%)",
                border:
                  "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 18%, transparent)",
              }}
            >
              <Clock3 size={16} strokeWidth={2.2} />
            </button>
          )}
        </div>
        {/* mb-1 optically centers the 40px buttons against the 48px single-line input (items-end row). */}
        {!isPreview && (
          <div className="relative mb-1 h-10 w-10 shrink-0">
            {/* Slide-up-to-lock target — purely a visual cue; locking is driven by the pointer leaving the mic. */}
            {isHold && (
              <div
                data-testid="composer-voice-lock-pill"
                className="ct-voice-lock-pill absolute bottom-full left-1/2 mb-7 flex -translate-x-1/2 flex-col items-center gap-1 rounded-full border px-2 py-2.5 text-[var(--ct-muted-fg)] shadow-[0_6px_20px_rgba(0,0,0,0.35)]"
                style={{
                  background: "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 90%, white 6%)",
                  borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 26%, transparent)",
                }}
              >
                <Lock size={14} strokeWidth={2.2} />
                <ChevronUp size={13} className="ct-voice-chevron-bounce" />
              </div>
            )}
            <button
              ref={buttonRef}
              type="button"
              data-testid={mainIsSend ? "composer-send-button" : "composer-voice-button"}
              title={mainTitle}
              onPointerDown={(e) => {
                if (mainIsSend || isLocked) {
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
                if (mainIsSend || isLocked) e.preventDefault();
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
                if (mainIsSend || isLocked) e.preventDefault();
              }}
              onClick={(e) => {
                e.preventDefault();
                if (isLocked) void stopToPreview();
              }}
              onContextMenu={(e) => {
                if (!mainIsSend || !onSchedule) return;
                e.preventDefault();
                onSchedule();
              }}
              className={`${btnBase} ${voiceActive ? "" : "ct-composer-round-button"} absolute left-1/2 top-1/2 rounded-full border text-white ${
                voiceActive
                  ? "z-30 shadow-[0_18px_55px_rgba(31,111,235,0.52)]"
                  : "h-10 w-10"
              } ${isLocked ? "ring-4 ring-[var(--ct-accent,#58a6ff)]/25" : ""} ${
                voiceActive || mainIsSend || isLocked
                  ? "shadow-[0_5px_18px_rgba(31,111,235,0.35)]"
                  : "shadow-[0_2px_10px_rgba(0,0,0,0.28)]"
              } disabled:opacity-40`}
              style={{
                background:
                  voiceActive || mainIsSend || isLocked
                    ? "linear-gradient(145deg, #4f8cff 0%, #1f6feb 100%)"
                    : "color-mix(in srgb, var(--ct-titlebar-bg, #161b22) 78%, white 9%)",
                borderColor:
                  voiceActive || mainIsSend || isLocked
                    ? "rgba(255,255,255,0.18)"
                    : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 24%, transparent)",
                color: voiceActive || mainIsSend || isLocked ? "white" : "var(--ct-fg, #c9d1d9)",
                height: voiceActive ? "5.5rem" : undefined,
                width: voiceActive ? "5.5rem" : undefined,
                // Constant center anchor in both states — only the size transitions, so growth radiates from the center.
                transform: "translate(-50%, -50%)",
                touchAction: "none",
              }}
              disabled={voiceBusy || (hasText && sendDisabled)}
            >
              {mainIsSend ? (
                <SendHorizontal size={20} fill="currentColor" />
              ) : isLocked ? (
                <Square size={18} fill="currentColor" />
              ) : (
                <Mic size={voiceActive ? 32 : 20} />
              )}
            </button>
          </div>
        )}
      </div>
    </>
  );
}
