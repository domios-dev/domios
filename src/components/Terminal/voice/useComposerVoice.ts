import { useEffect, useRef, useState, type PointerEvent } from "react";
import { bubblePoseFromPointer, type BubblePose } from "../composerBubble";
import { useVoiceRecorder } from "./useVoiceRecorder";

export type ComposerVoiceMode = "hold" | "locked" | "preview" | null;

function pointInRect(x: number, y: number, rect: DOMRect | undefined): boolean {
  return rect != null && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

export function useComposerVoice({
  hasText,
  onSend,
  onAttach,
  onSendWithAttachments,
  onError,
}: {
  hasText: boolean;
  onSend: () => void;
  onAttach: (paths: string[]) => void;
  onSendWithAttachments: (paths: string[]) => void;
  onError: (msg: string) => void;
}) {
  const buttonRef = useRef<HTMLButtonElement>(null);
  const shellRef = useRef<HTMLDivElement>(null);
  const cancelHintRef = useRef<HTMLDivElement>(null);

  const [voiceMode, setVoiceMode] = useState<ComposerVoiceMode>(null);
  const [voicePressed, setVoicePressed] = useState(false);
  const [voiceBusy, setVoiceBusy] = useState(false);
  const [bubblePose, setBubblePose] = useState<BubblePose>({ x: 0, y: 0, sx: 1, sy: 1 });
  const [cancelArmed, setCancelArmed] = useState(false);

  const cancelArmedRef = useRef(false);
  const voiceStartXRef = useRef<number | null>(null);
  const voiceStartYRef = useRef<number | null>(null);
  const voicePointerXRef = useRef<number | null>(null);
  const voicePointerYRef = useRef<number | null>(null);
  const voiceLockedFromDragRef = useRef(false);
  const activePointerIdRef = useRef<number | null>(null);
  const voiceModeRef = useRef<ComposerVoiceMode>(voiceMode);
  const voiceBusyRef = useRef(voiceBusy);
  const voice = useVoiceRecorder(onError);
  const voiceRecordingRef = useRef(voice.recording);

  useEffect(() => {
    voiceModeRef.current = voiceMode;
    voiceBusyRef.current = voiceBusy;
    voiceRecordingRef.current = voice.recording;
  }, [voiceMode, voiceBusy, voice.recording]);

  function armCancel(on: boolean) {
    cancelArmedRef.current = on;
    setCancelArmed(on);
  }

  function resetVoicePointer() {
    activePointerIdRef.current = null;
    voiceStartXRef.current = null;
    voiceStartYRef.current = null;
    voicePointerXRef.current = null;
    voicePointerYRef.current = null;
    voiceLockedFromDragRef.current = false;
    setBubblePose({ x: 0, y: 0, sx: 1, sy: 1 });
    armCancel(false);
  }

  function overCancelZone(x: number, y: number): boolean {
    return (
      pointInRect(x, y, shellRef.current?.getBoundingClientRect()) ||
      pointInRect(x, y, cancelHintRef.current?.getBoundingClientRect())
    );
  }

  function updateBubblePose(x: number, y: number) {
    if (voiceModeRef.current !== "hold" || !voiceRecordingRef.current) {
      setBubblePose({ x: 0, y: 0, sx: 1, sy: 1 });
      return;
    }
    const sx = voiceStartXRef.current;
    const sy = voiceStartYRef.current;
    if (sx == null || sy == null) return;
    setBubblePose(bubblePoseFromPointer(x, y, sx, sy));
  }

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
      setBubblePose({ x: 0, y: 0, sx: 1, sy: 1 });
      setVoiceMode("locked");
      return;
    }
    armCancel(left > 36 || overCancelZone(x, y));
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
    if (sendAfter) onSendWithAttachments(paths);
    else onAttach(paths);
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
      if (e.buttons !== 1) await finishVoice(true);
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
    updateBubblePose(e.clientX, e.clientY);
    evalVoiceDrag(e.clientX, e.clientY);
  }

  useEffect(() => {
    function onWindowPointerMove(e: globalThis.PointerEvent) {
      if (activePointerIdRef.current !== e.pointerId) return;
      voicePointerXRef.current = e.clientX;
      voicePointerYRef.current = e.clientY;
      if (!voiceRecordingRef.current) return;
      updateBubblePose(e.clientX, e.clientY);
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
      // voiceModeRef lags a render behind setVoiceMode; voiceLockedFromDragRef is
      // set synchronously, so a quick release right after drag-lock still latches.
      if (voiceModeRef.current === "locked" || voiceLockedFromDragRef.current) {
        setVoicePressed(false);
        return;
      }
      if (cancelArmedRef.current || overCancelZone(e.clientX, e.clientY)) {
        void cancelVoice();
        return;
      }
      void finishVoice(true);
    }

    function onWindowPointerCancel(e: globalThis.PointerEvent) {
      if (activePointerIdRef.current !== e.pointerId) return;
      if (voiceRecordingRef.current) setVoiceMode("locked");
      else {
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

  function lockFromPointerCancel() {
    if (voiceRecordingRef.current && voiceModeRef.current === "hold") {
      voiceLockedFromDragRef.current = true;
      setVoiceMode("locked");
    }
  }

  return {
    voice,
    shellRef,
    cancelHintRef,
    buttonRef,
    voiceMode,
    voicePressed,
    voiceBusy,
    bubblePose,
    cancelArmed,
    isHold,
    isLocked,
    isPreview,
    voiceActive,
    mainIsSend,
    composerHidden,
    mainTitle,
    startVoice,
    moveVoice,
    cancelVoice,
    stopToPreview,
    sendPreview,
    discardPreviewClip,
    lockFromPointerCancel,
  };
}

export type ComposerVoiceHandle = ReturnType<typeof useComposerVoice>;
