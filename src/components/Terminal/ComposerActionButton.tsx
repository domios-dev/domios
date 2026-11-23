import { useEffect, useRef, useState, type CSSProperties, type PointerEvent } from "react";
import { AudioLines, ChevronUp, Lock, Mic, Square, X } from "lucide-react";
import { bubblePoseFromPointer, type BubblePose } from "./composerBubble";
import type { ComposerVoiceHandle } from "./voice/useComposerVoice";
import type { LiveDictation } from "./voice/useLiveDictation";
import { DictationSetupPrompt } from "./voice/DictationSetupPrompt";
import { DictationStatus } from "./voice/DictationStatus";
import { isCancelGesture, isDragUpGesture } from "./voice/voiceFsm";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { SendIcon } from "../Icons";
import { COMPOSER_BUTTON_BASE } from "./composerButton";

const accentFg = "var(--ct-on-accent, #fff)";
const recordingFg = "var(--ct-red, #f85149)";
const lockedFg = "var(--ct-bg, #0d1117)";
const TAP_MS = 220;
const HOLD_VISUAL_DELAY_MS = 160;
const HOLD_DRAG_PX = 10;
const NEUTRAL_POSE: BubblePose = { x: 0, y: 0, sx: 1, sy: 1 };

interface DictationGesture {
  id: number;
  startX: number;
  startY: number;
  ptrX: number;
  ptrY: number;
  at: number;
  transcriptOnTap: boolean;
  started: boolean;
  locked: boolean;
  cancelArmed: boolean;
  holdVisual: boolean;
}

export function ComposerActionButton({
  voiceUi,
  dictation,
  onOpenLiveTranscript,
  liveTranscriptOnTap,
  spaceArming = false,
  spacePop = false,
  sendDisabled,
  sendUnavailableReason,
  onSend,
  onSchedule,
}: {
  voiceUi: ComposerVoiceHandle;
  dictation?: LiveDictation;
  onOpenLiveTranscript?: () => void;
  liveTranscriptOnTap?: boolean;
  spaceArming?: boolean;
  spacePop?: boolean;
  sendDisabled: boolean;
  sendUnavailableReason?: string | null;
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

  const gestureRef = useRef<DictationGesture | null>(null);
  const holdTimerRef = useRef<number | null>(null);
  const suppressNextPointerUpSendRef = useRef(false);
  const [dictationGesture, setDictationGesture] = useState<DictationGesture | null>(null);

  const useDictation = Boolean(dictation && !voiceActive);
  const dictationActive = Boolean(useDictation && dictation!.phase !== "idle");
  const dictationFinishing = Boolean(useDictation && dictation!.phase === "finishing");
  const transcriptOnTap = Boolean(liveTranscriptOnTap && onOpenLiveTranscript);

  function clearHoldTimer() {
    if (holdTimerRef.current !== null) {
      window.clearTimeout(holdTimerRef.current);
      holdTimerRef.current = null;
    }
  }

  function clearDictationGesture() {
    clearHoldTimer();
    gestureRef.current = null;
    setDictationGesture(null);
  }

  useEffect(() => {
    if (!dictationActive) clearDictationGesture();
  }, [dictationActive]);

  useEffect(() => clearHoldTimer, []);

  const effectiveSend = mainIsSend && !dictationActive && !voiceActive;
  const dictationHold = Boolean(useDictation && dictationGesture?.holdVisual && !dictationGesture.locked);
  const effectiveHold = isHold || dictationHold;
  const effectiveLocked = isLocked || Boolean(dictationActive && !dictationHold);
  const effectiveBubbleActive = voiceActive || dictationHold;
  const effectiveCancelArmed = voiceActive ? cancelArmed : Boolean(dictationGesture?.cancelArmed);
  const effectivePose = voiceActive
    ? bubblePose
    : dictationHold && dictationGesture
      ? bubblePoseFromPointer(dictationGesture.ptrX, dictationGesture.ptrY, dictationGesture.startX, dictationGesture.startY)
      : NEUTRAL_POSE;
  const effectiveTitle = sendUnavailableReason ?? (
    effectiveSend
      ? mainTitle
      : dictationActive
        ? (dictationFinishing ? "Finishing dictation" : "Stop dictation")
        : useDictation
          ? transcriptOnTap
            ? "Live Transcript · tap to start, hold to dictate"
            : onOpenLiveTranscript
              ? "Dictation · tap, hold, or slide up for Live Transcript"
              : "Dictation · tap, hold, or slide up to lock"
          : mainTitle
  );
  const effectiveAriaLabel = !effectiveSend && useDictation
    ? (dictationActive ? (dictationFinishing ? "Finishing dictation" : "Stop dictation") : transcriptOnTap ? "Start Live Transcript" : "Start dictation")
    : undefined;

  function handleDictationPointerDown(e: PointerEvent<HTMLButtonElement>) {
    if (!dictation || e.isPrimary === false || e.button !== 0) return;
    e.preventDefault();
    suppressNextPointerUpSendRef.current = true;
    if (dictation.active()) {
      clearDictationGesture();
      void dictation.stop();
      return;
    }
    const next: DictationGesture = {
      id: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      ptrX: e.clientX,
      ptrY: e.clientY,
      at: e.timeStamp,
      transcriptOnTap,
      started: !transcriptOnTap,
      locked: false,
      cancelArmed: false,
      holdVisual: false,
    };
    gestureRef.current = next;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    clearHoldTimer();
    holdTimerRef.current = window.setTimeout(() => {
      holdTimerRef.current = null;
      const current = gestureRef.current;
      if (!current || current.id !== next.id || current.locked || current.holdVisual) return;
      const engaged = { ...current, holdVisual: true, started: true };
      gestureRef.current = engaged;
      setDictationGesture(engaged);
      if (!current.started) void dictation.start();
    }, transcriptOnTap ? TAP_MS : HOLD_VISUAL_DELAY_MS);
    if (next.started) void dictation.start();
  }

  function handleDictationPointerMove(e: PointerEvent<HTMLButtonElement>) {
    const gesture = gestureRef.current;
    if (!dictation || !gesture || gesture.id !== e.pointerId || gesture.locked) return;
    const locked = isDragUpGesture(gesture.startX, gesture.startY, e.clientX, e.clientY);
    if (locked) {
      if (onOpenLiveTranscript) {
        clearDictationGesture();
        openFromGesture(gesture);
        return;
      }
      clearHoldTimer();
      const next = { ...gesture, ptrX: e.clientX, ptrY: e.clientY, locked: true, cancelArmed: false, holdVisual: false };
      gestureRef.current = next;
      setDictationGesture(next);
      return;
    }
    const cancel = isCancelGesture(gesture.startX, e.clientX, false);
    const moved = Math.hypot(e.clientX - gesture.startX, e.clientY - gesture.startY) >= HOLD_DRAG_PX;
    const holdVisual = gesture.holdVisual || moved || cancel;
    if (holdVisual) clearHoldTimer();
    const next = { ...gesture, ptrX: e.clientX, ptrY: e.clientY, cancelArmed: cancel, holdVisual, started: gesture.started || holdVisual };
    gestureRef.current = next;
    if (holdVisual || gesture.holdVisual) setDictationGesture(next);
    if (next.started && !gesture.started) void dictation.start();
  }

  function openFromGesture(gesture: DictationGesture) {
    if (!onOpenLiveTranscript) return;
    if (gesture.transcriptOnTap && gesture.started && dictation) {
      void Promise.resolve(dictation.cancel()).finally(onOpenLiveTranscript);
    } else {
      onOpenLiveTranscript();
    }
  }

  function handleDictationPointerUp(e: PointerEvent<HTMLButtonElement>) {
    const gesture = gestureRef.current;
    if (!dictation || !gesture || gesture.id !== e.pointerId) return;
    clearDictationGesture();
    if (gesture.locked) return;
    if (isDragUpGesture(gesture.startX, gesture.startY, e.clientX, e.clientY)) {
      openFromGesture(gesture);
      return;
    }
    const tap = e.timeStamp - gesture.at <= TAP_MS;
    if (gesture.cancelArmed || isCancelGesture(gesture.startX, e.clientX, false)) {
      if (gesture.started) void dictation.cancel();
    } else if (gesture.transcriptOnTap && !gesture.started && tap &&
      Math.hypot(e.clientX - gesture.startX, e.clientY - gesture.startY) < HOLD_DRAG_PX) {
      onOpenLiveTranscript?.();
    } else if (gesture.started && (!tap || gesture.transcriptOnTap)) {
      void dictation.stop();
    }
  }

  function handleDictationPointerAbort(e: PointerEvent<HTMLButtonElement>) {
    const gesture = gestureRef.current;
    if (!dictation || !gesture || gesture.id !== e.pointerId) return;
    clearDictationGesture();
    if (!gesture.locked && gesture.started) void dictation.cancel();
  }

  const showLiveTranscriptBubble = Boolean(
    dictationActive && !dictationFinishing && !effectiveHold && !voiceActive && !dictation?.setup && !dictation?.error && onOpenLiveTranscript,
  );

  return (
    <div className="ct-composer-action-shell ct-composer-control relative shrink-0">
      {effectiveHold && <VoiceLockPill liveTranscript={!voiceActive && Boolean(onOpenLiveTranscript)} />}
      {showLiveTranscriptBubble && onOpenLiveTranscript && (
        <AnchoredPopover open anchor={buttonRef} placement="top-end" gap={10}>
          <button
            type="button"
            data-testid="try-live-transcript-pill"
            onPointerDown={(e) => e.preventDefault()}
            onClick={() => {
              clearDictationGesture();
              onOpenLiveTranscript();
            }}
            className="ct-live-try-pill"
          >
            <AudioLines size={14} strokeWidth={2.2} className="ct-live-try-icon" aria-hidden="true" />
            <span>Try Live Transcript</span>
            <ChevronUp size={13} className="ct-live-try-chevron" aria-hidden="true" />
          </button>
        </AnchoredPopover>
      )}
      {!effectiveHold && !voiceActive && dictation?.setup && (
        <DictationSetupPrompt reason={dictation.setup} onDismiss={dictation.dismissSetup} anchor={buttonRef} />
      )}
      {!effectiveHold && !voiceActive && !dictation?.setup && dictation?.error && (
        <DictationStatus dictation={dictation} anchor={buttonRef} />
      )}
      <button
        ref={buttonRef}
        type="button"
        data-testid={effectiveSend ? "composer-send-button" : "composer-voice-button"}
        data-send={effectiveSend ? "true" : "false"}
        data-space-arming={spaceArming && !effectiveSend && !effectiveBubbleActive ? "true" : undefined}
        data-space-pop={spacePop && !effectiveSend ? "true" : undefined}
        aria-label={effectiveAriaLabel}
        aria-pressed={useDictation && !effectiveSend ? dictationActive : undefined}
        title={effectiveTitle}
        aria-description={sendUnavailableReason ?? undefined}
        aria-disabled={sendUnavailableReason ? "true" : undefined}
        onPointerDown={(e) => {
          if (sendUnavailableReason) {
            e.preventDefault();
            return;
          }
          if (effectiveSend) {
            suppressNextPointerUpSendRef.current = false;
            e.preventDefault();
            return;
          }
          if (useDictation) {
            handleDictationPointerDown(e);
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
        onPointerUp={(e) => {
          if (useDictation && gestureRef.current?.id === e.pointerId) {
            suppressNextPointerUpSendRef.current = false;
            handleDictationPointerUp(e);
            return;
          }
          if (suppressNextPointerUpSendRef.current) {
            suppressNextPointerUpSendRef.current = false;
            return;
          }
          // Secondary buttons open the schedule through onContextMenu instead of sending.
          if (e.button !== 0 || voice.recording || voicePressed || dictationActive) return;
          if (effectiveSend) onSend();
        }}
        onTouchStart={(e) => {
          if (effectiveSend || effectiveLocked || sendUnavailableReason) e.preventDefault();
        }}
        onPointerCancel={(e) => {
          suppressNextPointerUpSendRef.current = false;
          if (useDictation && gestureRef.current?.id === e.pointerId) {
            handleDictationPointerAbort(e);
            return;
          }
          if (voice.recording && voiceMode === "hold") lockFromPointerCancel();
        }}
        onLostPointerCapture={(e) => {
          if (useDictation && gestureRef.current?.id === e.pointerId) handleDictationPointerAbort(e);
        }}
        onPointerMove={(e) => {
          if (useDictation && gestureRef.current?.id === e.pointerId) {
            handleDictationPointerMove(e);
            return;
          }
          moveVoice(e);
        }}
        onTouchEnd={(e) => {
          if (effectiveSend || effectiveLocked || sendUnavailableReason) e.preventDefault();
        }}
        onClick={(e) => {
          e.preventDefault();
          if (e.detail !== 0 || sendUnavailableReason) return;
          if (effectiveSend && !sendDisabled) {
            onSend();
          } else if (useDictation && dictation && !dictationFinishing) {
            if (dictation.active()) void dictation.stop();
            else if (transcriptOnTap) onOpenLiveTranscript?.();
            else void dictation.start();
          }
        }}
        onContextMenu={(e) => {
          if (!effectiveSend || !onSchedule) return;
          e.preventDefault();
          onSchedule();
        }}
        className={`${COMPOSER_BUTTON_BASE} ct-composer-voice-bubble absolute left-1/2 top-1/2 rounded-full border ${
          effectiveBubbleActive ? "z-30 is-active" : "ct-composer-round-button ct-composer-control"
        } ${effectiveLocked ? "ring-4 ring-[var(--ct-accent,#58a6ff)]/25" : ""} ${
          spaceArming && !effectiveSend && !effectiveBubbleActive ? "ct-voice-space-arming" : ""
        } ${spacePop && !effectiveSend ? "ct-voice-space-pop" : ""} shadow-none disabled:opacity-40`}
        style={actionButtonStyle(effectiveBubbleActive, effectiveSend, effectiveLocked, effectivePose)}
        disabled={voiceBusy || dictationFinishing || (effectiveSend && sendDisabled && !sendUnavailableReason)}
      >
        <ActionButtonIcon
          isLocked={effectiveLocked}
          voiceActive={effectiveBubbleActive}
          cancelArmed={effectiveCancelArmed}
        />
      </button>
    </div>
  );
}

function VoiceLockPill({ liveTranscript = false }: { liveTranscript?: boolean }) {
  return (
    <div
      data-testid="composer-voice-lock-pill"
      className={`ct-voice-lock-pill absolute bottom-full left-1/2 mb-7 flex flex-col items-center gap-1.5 rounded-full border px-2 py-2.5 text-[var(--ct-muted-fg)] shadow-[0_6px_20px_rgba(0,0,0,0.35)]${liveTranscript ? " ct-voice-lock-pill--live" : ""}`}
      style={liveTranscript ? undefined : {
        background: "var(--ct-surface, #161b22)",
        borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 26%, transparent)",
      }}
    >
      {liveTranscript ? (
        <>
          <AudioLines size={14} strokeWidth={2.2} className="ct-live-try-icon" />
          <span className="ct-voice-lock-label">Live</span>
        </>
      ) : (
        <Lock size={14} strokeWidth={2.2} />
      )}
      <ChevronUp size={13} className="ct-voice-chevron-bounce" />
    </div>
  );
}

function ActionButtonIcon({
  isLocked,
  voiceActive,
  cancelArmed,
}: {
  isLocked: boolean;
  voiceActive: boolean;
  cancelArmed: boolean;
}) {
  // Armed to cancel: red cross. Locked: dark stop. Recording hold: red mic.
  if (cancelArmed) return <X size={30} strokeWidth={2.6} color={recordingFg} />;
  if (isLocked) return <Square size={18} fill={lockedFg} color={lockedFg} />;
  if (voiceActive) return <Mic size={30} strokeWidth={2.4} color={recordingFg} />;
  return (
    <>
      <span className="ct-send-bloom" aria-hidden />
      <span className="ct-send-glyph ct-send-glyph--mic" aria-hidden><Mic size={17} strokeWidth={2.2} /></span>
      <span className="ct-send-glyph ct-send-glyph--send" aria-hidden><SendIcon size={17} /></span>
    </>
  );
}

function actionButtonStyle(
  voiceActive: boolean,
  mainIsSend: boolean,
  isLocked: boolean,
  bubblePose: { x: number; y: number; sx: number; sy: number },
): CSSProperties & { "--ct-liquid-base-transform": string } {
  const accent = voiceActive || mainIsSend || isLocked;
  return {
    background: voiceActive || isLocked ? "var(--ct-accent)" : "var(--ct-surface, #161b22)",
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
