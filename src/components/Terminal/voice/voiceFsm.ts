// Pure FSM for composer voice interaction — no DOM, no React, fully unit-testable.

export type VoicePhase = "idle" | "starting" | "hold" | "locked" | "preview";

export interface VoiceState {
  phase: VoicePhase;
  pointerId: number | null;
  startX: number;
  startY: number;
  ptrX: number;
  ptrY: number;
  cancelArmed: boolean;
  busy: boolean;
  releasedDuringStart: boolean;
}

export type VoiceAction =
  | { type: "HANDS_FREE_START" }
  | { type: "HANDS_FREE_READY" }
  | { type: "POINTER_DOWN"; pointerId: number; x: number; y: number }
  | { type: "POINTER_MOVE"; pointerId: number; x: number; y: number; overCancelZone: boolean }
  | { type: "POINTER_UP"; pointerId: number }
  | { type: "POINTER_CANCEL"; pointerId: number }
  | { type: "START_OK" }
  | { type: "START_FAIL" }
  | { type: "BUSY_START" }
  | { type: "BUSY_END_IDLE" }
  | { type: "BUSY_END_PREVIEW" };

export const DRAG_UP_PX = 48;
export const DRAG_LEFT_PX = 36;

export function initVoiceState(): VoiceState {
  return {
    phase: "idle",
    pointerId: null,
    startX: 0,
    startY: 0,
    ptrX: 0,
    ptrY: 0,
    cancelArmed: false,
    busy: false,
    releasedDuringStart: false,
  };
}

export function isDragUpGesture(startX: number, startY: number, x: number, y: number): boolean {
  const up = startY - y;
  const left = startX - x;
  return up > DRAG_UP_PX && up >= left;
}

export function isCancelGesture(startX: number, x: number, overCancelZone: boolean): boolean {
  return startX - x > DRAG_LEFT_PX || overCancelZone;
}

export function voiceReducer(state: VoiceState, action: VoiceAction): VoiceState {
  const { phase, pointerId } = state;

  switch (action.type) {
    case "HANDS_FREE_START":
      return phase === "idle" ? { ...state, phase: "starting" } : state;
    case "HANDS_FREE_READY":
      return phase === "starting" ? { ...state, phase: "locked" } : state;
    case "POINTER_DOWN":
      if (phase !== "idle") return state;
      return {
        ...state,
        phase: "starting",
        pointerId: action.pointerId,
        startX: action.x,
        startY: action.y,
        ptrX: action.x,
        ptrY: action.y,
        cancelArmed: false,
        releasedDuringStart: false,
        busy: false,
      };

    case "POINTER_MOVE": {
      if (action.pointerId !== pointerId) return state;
      const { x, y } = action;
      if (phase === "starting") {
        return { ...state, ptrX: x, ptrY: y };
      }
      if (phase === "hold") {
        if (isDragUpGesture(state.startX, state.startY, x, y)) {
          return { ...state, phase: "locked", cancelArmed: false, ptrX: x, ptrY: y };
        }
        return {
          ...state,
          cancelArmed: isCancelGesture(state.startX, x, action.overCancelZone),
          ptrX: x,
          ptrY: y,
        };
      }
      return state;
    }

    case "POINTER_UP": {
      if (action.pointerId !== pointerId) return state;
      if (phase === "starting") {
        return { ...state, releasedDuringStart: true };
      }
      if (phase === "locked") {
        return state;
      }
      if (phase === "hold") {
        return { ...state, busy: true };
      }
      return state;
    }

    case "POINTER_CANCEL": {
      if (action.pointerId !== pointerId) return state;
      if (phase === "hold") {
        return { ...state, phase: "locked", cancelArmed: false };
      }
      if (phase === "starting") {
        return { ...state, releasedDuringStart: true };
      }
      return state;
    }

    case "START_OK": {
      if (phase !== "starting") return state;
      const wasLockGesture = isDragUpGesture(state.startX, state.startY, state.ptrX, state.ptrY);
      if (wasLockGesture) {
        return { ...state, phase: "locked", cancelArmed: false };
      }
      if (state.releasedDuringStart) {
        return { ...state, phase: "hold", busy: true };
      }
      return { ...state, phase: "hold" };
    }

    case "START_FAIL":
      if (phase !== "starting") return state;
      return initVoiceState();

    case "BUSY_START":
      return { ...state, busy: true };

    case "BUSY_END_IDLE":
      return initVoiceState();

    case "BUSY_END_PREVIEW":
      return { ...initVoiceState(), phase: "preview" };

    default:
      return state;
  }
}
