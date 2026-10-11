export type LarkCapturePhase =
  | "disabled"
  | "armed"
  | "starting"
  | "recording"
  | "stopping"
  | "transcribing"
  | "error";

export type LarkInvalidationReason =
  | "disabled"
  | "grantRevoked"
  | "unregistered"
  | "deviceSelectionChanged"
  | "deviceRemoved"
  | "accessLost"
  | "deviceUnavailable"
  | "cancelled";

export interface LarkCaptureLease {
  lease: string;
  pluginId: string;
  tabId: string;
  microphoneId: string;
  generation: number;
}

export interface LarkCaptureInvalidation {
  pluginId: string;
  generation: number;
  reason: LarkInvalidationReason;
}

export interface LarkRecordingStatus {
  pluginId: string;
  granted: boolean;
  enabled: boolean;
  available: boolean;
  generation: number;
}

export interface LarkCaptureState {
  phase: LarkCapturePhase;
  operation: number;
  pluginId?: string;
  generation?: number;
  capture?: LarkCaptureLease;
  stopIntent?: "send" | "discard";
  invalidation?: LarkInvalidationReason;
  recorderStarted: boolean;
  recorderStopped: boolean;
  tracksReleased: boolean;
  hostCleanupAcknowledged: boolean;
  error?: string;
}

export type LarkCaptureAction =
  | { type: "STATUS"; status: LarkRecordingStatus }
  | { type: "START"; capture: LarkCaptureLease }
  | { type: "RECORDER_STARTED"; operation: number }
  | { type: "STOP"; operation: number; intent: "send" | "discard"; error?: string }
  | { type: "CANCEL"; operation: number; message: string; reason?: LarkInvalidationReason }
  | { type: "INVALIDATED"; event: LarkCaptureInvalidation }
  | { type: "HOST_CLEANUP_ACK"; operation: number }
  | { type: "RECORDER_STOPPED"; operation: number }
  | { type: "TRACKS_RELEASED"; operation: number }
  | { type: "COMMIT_SUCCEEDED"; operation: number }
  | { type: "FAILED"; operation: number; message: string }
  | { type: "RESET" };

export function initialLarkCaptureState(): LarkCaptureState {
  return {
    phase: "disabled",
    operation: 0,
    recorderStarted: false,
    recorderStopped: false,
    tracksReleased: false,
    hostCleanupAcknowledged: false,
  };
}

function settleStop(state: LarkCaptureState): LarkCaptureState {
  if (state.phase !== "stopping" || !state.recorderStopped || !state.tracksReleased || !state.hostCleanupAcknowledged) return state;
  if (state.invalidation) {
    return { ...state, phase: "error", capture: undefined, error: invalidationMessage(state.invalidation) };
  }
  if (state.error) {
    return { ...state, phase: "error", capture: undefined };
  }
  if (state.stopIntent === "send" && state.recorderStarted) {
    return { ...state, phase: "transcribing" };
  }
  return {
    ...state,
    phase: state.pluginId ? "armed" : "disabled",
    capture: undefined,
    stopIntent: undefined,
  };
}

export function larkCaptureTransition(
  state: LarkCaptureState,
  action: LarkCaptureAction,
): LarkCaptureState {
  switch (action.type) {
    case "STATUS": {
      const { status } = action;
      if (state.phase === "starting" || state.phase === "recording" || state.phase === "stopping" || state.phase === "transcribing") {
        return state;
      }
      if (status.granted && status.enabled && status.available) {
        return {
          ...state,
          phase: "armed",
          pluginId: status.pluginId,
          generation: status.generation,
          capture: undefined,
          error: undefined,
          invalidation: undefined,
        };
      }
      return {
        ...state,
        phase: "disabled",
        pluginId: status.pluginId,
        generation: status.generation,
        capture: undefined,
        error: undefined,
        invalidation: undefined,
      };
    }
    case "START": {
      const { capture } = action;
      if (!capture.lease || !capture.pluginId || !capture.tabId || !capture.microphoneId || !Number.isSafeInteger(capture.generation)) {
        return state;
      }
      if (state.phase !== "armed" || state.pluginId !== capture.pluginId || state.generation !== capture.generation) {
        return state;
      }
      return {
        phase: "starting",
        operation: state.operation + 1,
        pluginId: capture.pluginId,
        generation: capture.generation,
        capture,
        recorderStarted: false,
        recorderStopped: false,
        tracksReleased: false,
        hostCleanupAcknowledged: false,
      };
    }
    case "RECORDER_STARTED":
      if (action.operation !== state.operation || state.phase !== "starting") return state;
      return { ...state, phase: "recording", recorderStarted: true };
    case "STOP":
      if (action.operation !== state.operation || !state.capture) return state;
      if (state.phase === "stopping" || state.phase === "transcribing" || state.phase === "error") return state;
      return settleStop({
        ...state,
        phase: "stopping",
        stopIntent: state.phase === "recording" ? action.intent : "discard",
        error: action.error,
      });
    case "CANCEL":
      if (action.operation !== state.operation || !state.capture) return state;
      if (state.phase === "error") return state;
      if (state.phase === "transcribing") {
        return {
          ...state,
          phase: "error",
          capture: undefined,
          invalidation: action.reason,
          error: action.message,
        };
      }
      return settleStop({
        ...state,
        phase: "stopping",
        stopIntent: "discard",
        invalidation: action.reason ?? state.invalidation,
        error: action.message,
      });
    case "INVALIDATED":
      if (!matchesCaptureInvalidation(state, action.event)) return state;
      return invalidateState(state, action.event);
    case "HOST_CLEANUP_ACK":
      if (action.operation !== state.operation || state.phase !== "stopping") return state;
      return settleStop({ ...state, hostCleanupAcknowledged: true });
    case "RECORDER_STOPPED":
      if (action.operation !== state.operation || state.phase !== "stopping") return state;
      return settleStop({ ...state, recorderStopped: true });
    case "TRACKS_RELEASED":
      if (action.operation !== state.operation || state.phase !== "stopping") return state;
      return settleStop({ ...state, tracksReleased: true });
    case "COMMIT_SUCCEEDED":
      if (action.operation !== state.operation || state.phase !== "transcribing" || state.invalidation) return state;
      return {
        ...state,
        phase: "armed",
        capture: undefined,
        stopIntent: undefined,
        recorderStarted: false,
        recorderStopped: false,
        tracksReleased: false,
      };
    case "FAILED":
      if (action.operation !== state.operation) return state;
      if (state.phase === "stopping") return settleStop({ ...state, error: action.message });
      return { ...state, phase: "error", capture: undefined, error: action.message };
    case "RESET":
      return { ...initialLarkCaptureState(), operation: state.operation + 1 };
  }
}

function invalidateState(state: LarkCaptureState, event: LarkCaptureInvalidation): LarkCaptureState {
  if (state.invalidation) return state;
  if (state.phase === "transcribing") {
    return {
      ...state,
      phase: "error",
      capture: undefined,
      hostCleanupAcknowledged: true,
      invalidation: event.reason,
      error: invalidationMessage(event.reason),
    };
  }
  if (state.phase === "error") return state;
  return {
    ...state,
    phase: "stopping",
    stopIntent: "discard",
    hostCleanupAcknowledged: true,
    invalidation: event.reason,
    error: invalidationMessage(event.reason),
  };
}

export function matchesCaptureInvalidation(
  state: LarkCaptureState,
  event: LarkCaptureInvalidation,
): boolean {
  return state.capture?.pluginId === event.pluginId && state.capture.generation === event.generation;
}

export function invalidationMessage(reason: LarkInvalidationReason): string {
  switch (reason) {
    case "deviceRemoved": return "The selected LARK device was removed.";
    case "accessLost": return "Microphone or device access was lost.";
    case "deviceUnavailable": return "The selected LARK device became unavailable.";
    case "deviceSelectionChanged": return "The selected LARK device changed.";
    case "grantRevoked": return "Local recording permission was revoked.";
    case "unregistered": return "The LARK recording plugin was unloaded.";
    case "disabled": return "LARK recording was disabled.";
    case "cancelled": return "Voice recording was cancelled.";
  }
}

export function exactMicrophoneConstraints(deviceId: string): MediaStreamConstraints {
  if (!deviceId.trim()) throw new Error("A selected microphone is required.");
  return { audio: { deviceId: { exact: deviceId } } };
}

export function hasExactLiveMicrophone(stream: MediaStream, deviceId: string): boolean {
  const tracks = stream.getAudioTracks();
  return tracks.length === 1
    && tracks[0].readyState === "live"
    && tracks[0].getSettings().deviceId === deviceId;
}

export type CaptureOwner = "manual" | "lark";
let captureOwner: CaptureOwner | null = null;

export function claimCaptureOwnership(owner: CaptureOwner): boolean {
  if (captureOwner !== null) return false;
  captureOwner = owner;
  return true;
}

export function releaseCaptureOwnership(owner: CaptureOwner): void {
  if (captureOwner === owner) captureOwner = null;
}

export function currentCaptureOwner(): CaptureOwner | null {
  return captureOwner;
}

export function resetCaptureOwnershipForTest(): void {
  captureOwner = null;
}
