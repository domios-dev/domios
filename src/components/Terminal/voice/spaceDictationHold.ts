export const SPACE_DICTATION_HOLD_MS = 300;
export const SPACE_DICTATION_POP_MS = 260;

export type SpaceDictationPhase = "idle" | "arming" | "recording";

export interface SpaceDictationKey {
  code: string;
  repeat: boolean;
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  isComposing: boolean;
}

export interface SpaceDictationContext {
  empty: boolean;
  dictating: boolean;
  pickerOpen: boolean;
  composing: boolean;
}

export interface SpaceDictationPorts {
  onPhaseChange: (phase: SpaceDictationPhase) => void;
  onStartDictation: () => void;
  onStopDictation: () => void;
  onInsertSpace: () => void;
  onPop: () => void;
  setTimer?: (callback: () => void, ms: number) => ReturnType<typeof setTimeout>;
  clearTimer?: (id: ReturnType<typeof setTimeout>) => void;
}

/**
 * Pure controller for 300ms hold-Space push-to-talk in an empty composer.
 * Swallows OS key-repeat while arming/recording, inserts a normal space on a quick tap (<300ms),
 * ignites dictation at 300ms with a tactile pop, and stops cleanly on keyup or blur.
 */
export function createSpaceDictationHold(ports: SpaceDictationPorts) {
  const schedule = ports.setTimer ?? ((cb, ms) => setTimeout(cb, ms));
  const cancel = ports.clearTimer ?? ((id) => clearTimeout(id));
  let phase: SpaceDictationPhase = "idle";
  let armTimer: ReturnType<typeof setTimeout> | null = null;

  function clearArmTimer() {
    if (armTimer !== null) {
      cancel(armTimer);
      armTimer = null;
    }
  }

  function setPhase(next: SpaceDictationPhase) {
    if (phase === next) return;
    phase = next;
    ports.onPhaseChange(next);
  }

  function onKeyDown(key: SpaceDictationKey, context: SpaceDictationContext): boolean {
    if (key.code !== "Space") {
      if (phase === "arming") {
        clearArmTimer();
        setPhase("idle");
      }
      return false;
    }
    if (phase === "arming" || phase === "recording") return true;
    const unmodified = !key.shiftKey && !key.ctrlKey && !key.metaKey && !key.altKey;
    if (key.repeat || !unmodified || key.isComposing || !context.empty || context.dictating || context.pickerOpen || context.composing) {
      return false;
    }
    setPhase("arming");
    armTimer = schedule(() => {
      armTimer = null;
      if (phase !== "arming") return;
      setPhase("recording");
      ports.onPop();
      ports.onStartDictation();
    }, SPACE_DICTATION_HOLD_MS);
    return true;
  }

  function onKeyUp(code: string, focused: boolean) {
    if (code !== "Space") return;
    if (phase === "arming") {
      clearArmTimer();
      setPhase("idle");
      if (focused) ports.onInsertSpace();
      return;
    }
    if (phase === "recording") {
      setPhase("idle");
      ports.onStopDictation();
    }
  }

  function onBlur() {
    if (phase === "arming") {
      clearArmTimer();
      setPhase("idle");
      return;
    }
    if (phase === "recording") {
      setPhase("idle");
      ports.onStopDictation();
    }
  }

  function resetIfDictationEnded() {
    if (phase === "recording") setPhase("idle");
  }

  function dispose() {
    clearArmTimer();
    const wasRecording = phase === "recording";
    setPhase("idle");
    if (wasRecording) ports.onStopDictation();
  }

  return {
    phase: () => phase,
    onKeyDown,
    onKeyUp,
    onBlur,
    resetIfDictationEnded,
    dispose,
  };
}
