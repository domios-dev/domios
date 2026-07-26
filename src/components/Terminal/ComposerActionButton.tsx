import type { CSSProperties } from "react";
import { ChevronUp, Lock, Mic, Square, X } from "lucide-react";
import { ComposerSendIcon } from "./composerBubble";
import type { ComposerVoiceHandle } from "./voice/useComposerVoice";

const btnBase =
  "ct-liquid-interactive flex items-center justify-center shrink-0";

const accentFg = "var(--ct-on-accent, #fff)";
const recordingFg = "var(--ct-red, #f85149)";
const lockedFg = "var(--ct-bg, #0d1117)";

export function ComposerActionButton({
  voiceUi,
  sendDisabled,
  onSend,
  onSchedule,
}: {
  voiceUi: ComposerVoiceHandle;
  sendDisabled: boolean;
  onSend: () => void;
  onSchedule?: () => void;
}) {
  const {
    voice,
    buttonRef,
    voiceMode,
    voiceBusy,
    bubblePose,
    isHold,
    isLocked,
    voiceActive,
    mainIsSend,
    mainTitle,
    voicePressed,
    cancelArmed,
    startVoice,
    moveVoice,
    stopToPreview,
    lockFromPointerCancel,
  } = voiceUi;

  return (
    <div className="ct-composer-action-shell relative shrink-0 h-10 w-10">
      {isHold && <VoiceLockPill />}
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
          // completes drag-to-lock is a pointerup, so it can't trigger this.
          if (isLocked) {
            e.preventDefault();
            void stopToPreview();
            return;
          }
          void startVoice(e);
        }}
        onPointerUp={() => {
          if (voice.recording || voicePressed) return;
          if (mainIsSend) onSend();
        }}
        onTouchStart={(e) => {
          if (mainIsSend || isLocked) e.preventDefault();
        }}
        onPointerCancel={() => {
          if (voice.recording && voiceMode === "hold") lockFromPointerCancel();
        }}
        onPointerMove={moveVoice}
        onTouchEnd={(e) => {
          if (mainIsSend || isLocked) e.preventDefault();
        }}
        onClick={(e) => {
          e.preventDefault();
        }}
        onContextMenu={(e) => {
          if (!mainIsSend || !onSchedule) return;
          e.preventDefault();
          onSchedule();
        }}
        className={`${btnBase} ct-composer-voice-bubble absolute left-1/2 top-1/2 rounded-full border ${
          voiceActive ? "z-30 is-active" : "ct-composer-round-button h-10 w-10"
        } ${isLocked ? "ring-4 ring-[var(--ct-accent,#58a6ff)]/25" : ""} shadow-none disabled:opacity-40`}
        style={actionButtonStyle(voiceActive, mainIsSend, isLocked, bubblePose)}
        disabled={voiceBusy || (mainIsSend && sendDisabled)}
      >
        <ActionButtonIcon
          mainIsSend={mainIsSend}
          isLocked={isLocked}
          voiceActive={voiceActive}
          cancelArmed={cancelArmed}
        />
      </button>
    </div>
  );
}

function VoiceLockPill() {
  return (
    <div
      data-testid="composer-voice-lock-pill"
      className="ct-voice-lock-pill absolute bottom-full left-1/2 mb-7 flex -translate-x-1/2 flex-col items-center gap-1 rounded-full border px-2 py-2.5 text-[var(--ct-muted-fg)] shadow-[0_6px_20px_rgba(0,0,0,0.35)]"
      style={{
        background: "var(--ct-surface, #161b22)",
        borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 26%, transparent)",
      }}
    >
      <Lock size={14} strokeWidth={2.2} />
      <ChevronUp size={13} className="ct-voice-chevron-bounce" />
    </div>
  );
}

function ActionButtonIcon({
  mainIsSend,
  isLocked,
  voiceActive,
  cancelArmed,
}: {
  mainIsSend: boolean;
  isLocked: boolean;
  voiceActive: boolean;
  cancelArmed: boolean;
}) {
  if (mainIsSend) return <ComposerSendIcon size={20} />;
  // Armed to cancel: red cross. Locked: dark pause. Recording: red mic.
  if (cancelArmed) return <X size={30} strokeWidth={2.6} color={recordingFg} />;
  if (isLocked) return <Square size={18} fill={lockedFg} color={lockedFg} />;
  if (voiceActive) return <Mic size={30} strokeWidth={2.4} color={recordingFg} />;
  return <Mic size={20} strokeWidth={2.2} />;
}

function actionButtonStyle(
  voiceActive: boolean,
  mainIsSend: boolean,
  isLocked: boolean,
  bubblePose: { x: number; y: number; sx: number; sy: number },
): CSSProperties & { "--ct-liquid-base-transform": string } {
  const accent = voiceActive || mainIsSend || isLocked;
  return {
    background: mainIsSend
      ? "color-mix(in srgb, var(--ct-accent) 78%, var(--ct-bg))"
      : accent
        ? "var(--ct-accent)"
        : "var(--ct-surface, #161b22)",
    border: accent
      ? "1px solid color-mix(in srgb, var(--ct-on-accent, #fff) 22%, var(--ct-accent))"
      : "1px solid var(--ct-border)",
    color: accent ? accentFg : "var(--ct-fg, #c9d1d9)",
    height: voiceActive ? "5.5rem" : undefined,
    width: voiceActive ? "5.5rem" : undefined,
    "--ct-liquid-base-transform": voiceActive
      ? `translate(calc(-50% + ${bubblePose.x}px), calc(-50% + ${bubblePose.y}px)) scale(${bubblePose.sx}, ${bubblePose.sy})`
      : "translate(-50%, -50%)",
    touchAction: "none" as const,
  };
}
