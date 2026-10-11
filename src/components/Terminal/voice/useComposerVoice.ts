import { useEffect, useRef, useState, type PointerEvent } from "react";
import { bubblePoseFromPointer, type BubblePose } from "../composerBubble";
import { useVoiceRecorder } from "./useVoiceRecorder";
import { registerComposerRecording } from "./composerRecording";
import {
  voiceReducer,
  initVoiceState,
  isDragUpGesture,
  type VoiceState,
  type VoiceAction,
} from "./voiceFsm";

export type ComposerVoiceMode = "hold" | "locked" | "preview" | null;

function pointInRect(x: number, y: number, rect: DOMRect | undefined): boolean {
  return rect != null && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

export function useComposerVoice({
  tabId,
  hasText,
  onSend,
  onAttach,
  onSendWithAttachments,
  onError,
}: {
  tabId?: string;
  hasText: boolean;
  onSend: () => void;
  onAttach: (paths: string[]) => void;
  onSendWithAttachments: (paths: string[]) => void;
  onError: (msg: string) => void;
}) {
  const buttonRef = useRef<HTMLButtonElement>(null);
  const shellRef = useRef<HTMLDivElement>(null);
  const cancelHintRef = useRef<HTMLDivElement>(null);

  // Single source of truth: stateRef is updated SYNCHRONOUSLY on every dispatch.
  // Window handlers read stateRef.current — never a render-stale mirror.
  const stateRef = useRef<VoiceState>(initVoiceState());
  const [, forceRender] = useState(0);

  const hardwareRef = useRef<{ cancel: () => void; stop: () => void } | null>(null);
  const voice = useVoiceRecorder((message) => {
    onError(message);
    hardwareRef.current?.cancel();
  });
  // Ref so window-handler closures always call the latest voice methods.
  const voiceRef = useRef(voice);
  voiceRef.current = voice;

  // Callback refs to avoid stale closures in the empty-deps window useEffect.
  const cbsRef = useRef({ onSend, onAttach, onSendWithAttachments });
  cbsRef.current = { onSend, onAttach, onSendWithAttachments };

  function dispatch(action: VoiceAction) {
    stateRef.current = voiceReducer(stateRef.current, action);
    forceRender((n) => n + 1);
  }

  function overCancelZone(x: number, y: number): boolean {
    return (
      pointInRect(x, y, shellRef.current?.getBoundingClientRect()) ||
      pointInRect(x, y, cancelHintRef.current?.getBoundingClientRect())
    );
  }

  function releaseCapture(pointerId: number) {
    const button = buttonRef.current;
    if (button?.hasPointerCapture(pointerId)) {
      try {
        button.releasePointerCapture(pointerId);
      } catch {
        /* already released */
      }
    }
  }

  async function runFinishVoice(sendAfter: boolean) {
    const paths = await voiceRef.current.stop();
    dispatch({ type: "BUSY_END_IDLE" });
    if (paths.length === 0) return;
    if (sendAfter) cbsRef.current.onSendWithAttachments(paths);
    else cbsRef.current.onAttach(paths);
  }

  async function runCancelVoice() {
    await voiceRef.current.stop(true);
    dispatch({ type: "BUSY_END_IDLE" });
  }

  async function startVoice(e: PointerEvent<HTMLButtonElement>) {
    if (hasText) {
      onSend();
      return;
    }
    if (stateRef.current.phase !== "idle") return;

    dispatch({ type: "POINTER_DOWN", pointerId: e.pointerId, x: e.clientX, y: e.clientY });
    e.currentTarget.setPointerCapture(e.pointerId);

    const started = await voice.start();

    if (started) {
      const s = stateRef.current;
      dispatch({ type: "START_OK" });
      // If pointer released during warmup without a drag-up gesture → quick tap, finish now.
      if (s.releasedDuringStart && !isDragUpGesture(s.startX, s.startY, s.ptrX, s.ptrY)) {
        void runFinishVoice(true);
      }
      // isDragUpGesture during warmup → reducer already locked; keep recording hands-free.
      // !releasedDuringStart → now in hold; user still holding.
    } else {
      dispatch({ type: "START_FAIL" });
      releaseCapture(e.pointerId);
    }
  }

  function moveVoice(e: PointerEvent<HTMLButtonElement>) {
    dispatch({
      type: "POINTER_MOVE",
      pointerId: e.pointerId,
      x: e.clientX,
      y: e.clientY,
      overCancelZone: overCancelZone(e.clientX, e.clientY),
    });
  }

  async function stopToPreview() {
    if (hardwareRef.current) {
      hardwareRef.current.stop();
      return;
    }
    const s = stateRef.current;
    if (s.busy || s.phase !== "locked") return;
    dispatch({ type: "BUSY_START" });
    const hasClip = await voice.stopForPreview();
    dispatch(hasClip ? { type: "BUSY_END_PREVIEW" } : { type: "BUSY_END_IDLE" });
  }

  async function sendPreview() {
    const s = stateRef.current;
    if (s.busy || s.phase !== "preview") return;
    dispatch({ type: "BUSY_START" });
    const paths = await voice.commitPreview();
    dispatch({ type: "BUSY_END_IDLE" });
    if (paths.length > 0) onSendWithAttachments(paths);
  }

  function discardPreviewClip() {
    voice.discardPreview();
    dispatch({ type: "BUSY_END_IDLE" });
  }

  function cancelVoice() {
    if (hardwareRef.current) {
      hardwareRef.current.cancel();
      return;
    }
    if (stateRef.current.busy) return;
    dispatch({ type: "BUSY_START" });
    void runCancelVoice();
  }

  function lockFromPointerCancel() {
    const s = stateRef.current;
    if (voiceRef.current.recording && s.phase === "hold" && s.pointerId !== null) {
      dispatch({ type: "POINTER_CANCEL", pointerId: s.pointerId });
    }
  }

  useEffect(() => {
    if (!tabId) return;
    const unregister = registerComposerRecording(tabId, {
      async start(microphoneId, runtime, cancel, stop) {
        if (stateRef.current.phase !== "idle") return false;
        hardwareRef.current = { cancel, stop };
        dispatch({ type: "HANDS_FREE_START" });
        const started = await voiceRef.current.start({ microphoneId, runtime });
        dispatch({ type: started ? "HANDS_FREE_READY" : "START_FAIL" });
        if (!started) hardwareRef.current = null;
        return started;
      },
      async stop(discard) {
        dispatch({ type: "BUSY_START" });
        try {
          return await voiceRef.current.stopClip(discard);
        } finally {
          hardwareRef.current = null;
          dispatch({ type: "BUSY_END_IDLE" });
        }
      },
    });
    return () => {
      unregister();
      hardwareRef.current?.cancel();
    };
  }, [tabId]);

  useEffect(() => {
    function onWindowPointerMove(e: globalThis.PointerEvent) {
      if (stateRef.current.pointerId !== e.pointerId) return;
      dispatch({
        type: "POINTER_MOVE",
        pointerId: e.pointerId,
        x: e.clientX,
        y: e.clientY,
        overCancelZone: overCancelZone(e.clientX, e.clientY),
      });
    }

    function onWindowPointerUp(e: globalThis.PointerEvent) {
      const s = stateRef.current;
      if (s.pointerId !== e.pointerId) return;
      releaseCapture(e.pointerId);

      if (s.phase === "locked") {
        // Releasing pointer in locked is a no-op — hands-free recording continues.
        return;
      }
      if (s.phase === "starting") {
        dispatch({ type: "POINTER_UP", pointerId: e.pointerId });
        return;
      }
      if (s.phase === "hold") {
        // Read cancelArmed from current state BEFORE dispatch (synchronous, no stale read).
        const shouldCancel = s.cancelArmed || overCancelZone(e.clientX, e.clientY);
        dispatch({ type: "POINTER_UP", pointerId: e.pointerId });
        if (shouldCancel) void runCancelVoice();
        else void runFinishVoice(true);
      }
    }

    function onWindowPointerCancel(e: globalThis.PointerEvent) {
      if (stateRef.current.pointerId !== e.pointerId) return;
      dispatch({ type: "POINTER_CANCEL", pointerId: e.pointerId });
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

  const state = stateRef.current;
  const voiceMode: ComposerVoiceMode =
    state.phase === "hold" ? "hold" :
    state.phase === "locked" ? "locked" :
    state.phase === "preview" ? "preview" : null;
  const voicePressed = state.phase === "starting" || state.phase === "hold";
  const voiceBusy = state.busy;
  const isHold = state.phase === "hold";
  const isLocked = state.phase === "locked";
  const isPreview = state.phase === "preview";
  const voiceActive = voicePressed || voice.recording;
  const composerHidden = voice.recording || isPreview;
  const mainIsSend = hasText;
  const cancelArmed = state.cancelArmed;
  const mainTitle = mainIsSend
    ? "Send"
    : isLocked
      ? "Stop and review voice note"
      : "Hold to record voice note";
  const bubblePose: BubblePose =
    state.phase === "hold" && voice.recording
      ? bubblePoseFromPointer(state.ptrX, state.ptrY, state.startX, state.startY)
      : { x: 0, y: 0, sx: 1, sy: 1 };

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
