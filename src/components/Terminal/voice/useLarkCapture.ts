import { useCallback, useEffect, useRef, useState } from "react";
import { markVoice } from "@codeterm/chat-core";
import type { Tab } from "../../../lib/types";
import { invoke, listen, type UnlistenFn } from "../../../lib/bridge";
import { IS_WEB } from "../../../lib/webApi";
import { addChatSend, observeDeliveryStatus, removeChatSend } from "../chatSendStore";
import type { ChatSendStatus } from "../chatSendStatus";
import {
  MIN_VOICE_BYTES,
  microphoneErrorMessage,
  pickAudioMime,
  storeLarkCaptureClip,
  discardLarkCaptureClip,
} from "./audio";
import {
  claimCaptureOwnership,
  exactMicrophoneConstraints,
  hasExactLiveMicrophone,
  initialLarkCaptureState,
  larkCaptureTransition,
  releaseCaptureOwnership,
  type LarkCaptureAction,
  type LarkCaptureInvalidation,
  type LarkCaptureLease,
  type LarkCaptureState,
  type LarkRecordingStatus,
} from "./larkCaptureModel";

const MAX_CAPTURE_MS = 120_000;

type CaptureEvent =
  | { kind: "start"; capture: LarkCaptureLease }
  | { kind: "stop"; lease: string };

type Deadline = (callback: () => void, ms: number) => () => void;

export interface LarkCaptureRuntime {
  isDesktop: boolean;
  listen: <T>(event: string, handler: (event: { payload: T }) => void) => Promise<UnlistenFn>;
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  getUserMedia: (constraints: MediaStreamConstraints) => Promise<MediaStream>;
  createRecorder: (stream: MediaStream, mime: string | undefined) => MediaRecorder;
  storeClip: (lease: string, blob: Blob, mime: string) => Promise<{ handle: string; path: string }>;
  discardClip: (lease: string, clipHandle: string) => Promise<void>;
  scheduleDeadline: Deadline;
  now: () => number;
  createId: () => string;
};

export interface UseLarkCaptureOptions {
  tabs: Record<string, Tab>;
  runtime?: Partial<LarkCaptureRuntime>;
}

type LocalRecordingCommitOutcome =
  | { kind: "tracked"; status: ChatSendStatus }
  | { kind: "preHandoffFailure"; message: string }
  | { kind: "possibleHandoff"; message: string };

function defaultRecorder(stream: MediaStream, mime: string | undefined): MediaRecorder {
  if (!mime) return new MediaRecorder(stream);
  try {
    return new MediaRecorder(stream, { mimeType: mime });
  } catch {
    return new MediaRecorder(stream);
  }
}

function defaultDeadline(callback: () => void, ms: number): () => void {
  const id = window.setTimeout(callback, ms);
  return () => window.clearTimeout(id);
}

function tabBinding(tab: Tab | undefined): string | null {
  if (!tab) return null;
  return JSON.stringify([
    tab.id,
    tab.tab_type,
    tab.pty_id,
    tab.backing,
    tab.agent_session_id,
    tab.session_binding_nonce,
  ]);
}

function statusPayload(value: unknown): value is LarkRecordingStatus {
  if (!value || typeof value !== "object") return false;
  const status = value as Record<string, unknown>;
  return typeof status.pluginId === "string"
    && typeof status.granted === "boolean"
    && typeof status.enabled === "boolean"
    && typeof status.available === "boolean"
    && Number.isSafeInteger(status.generation);
}

function captureEvent(value: unknown): value is CaptureEvent {
  if (!value || typeof value !== "object") return false;
  const event = value as Record<string, unknown>;
  if (event.kind === "stop") return typeof event.lease === "string";
  if (event.kind !== "start" || !event.capture || typeof event.capture !== "object") return false;
  const capture = event.capture as Record<string, unknown>;
  return typeof capture.lease === "string"
    && typeof capture.pluginId === "string"
    && typeof capture.tabId === "string"
    && typeof capture.microphoneId === "string"
    && Number.isSafeInteger(capture.generation);
}

function invalidationPayload(value: unknown): value is LarkCaptureInvalidation {
  if (!value || typeof value !== "object") return false;
  const event = value as Record<string, unknown>;
  return typeof event.pluginId === "string"
    && Number.isSafeInteger(event.generation)
    && ["disabled", "grantRevoked", "unregistered", "deviceSelectionChanged", "deviceRemoved", "accessLost", "deviceUnavailable", "cancelled"].includes(String(event.reason));
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function microphoneFailureReason(error: unknown): LarkCaptureInvalidation["reason"] {
  if (error instanceof DOMException && ["NotAllowedError", "SecurityError"].includes(error.name)) return "accessLost";
  if (error instanceof DOMException && error.name === "NotFoundError") return "deviceRemoved";
  return "deviceUnavailable";
}

export function useLarkCapture({ tabs, runtime: runtimeOverrides }: UseLarkCaptureOptions) {
  const [state, setState] = useState<LarkCaptureState>(initialLarkCaptureState);
  const listenerReadyRef = useRef<Promise<void> | null>(null);
  const resolveListenerReadyRef = useRef<(() => void) | null>(null);
  if (!listenerReadyRef.current) {
    listenerReadyRef.current = new Promise<void>((resolve) => {
      resolveListenerReadyRef.current = resolve;
    });
  }
  const stateRef = useRef(state);
  const tabsRef = useRef(tabs);
  const runtimeRef = useRef<LarkCaptureRuntime>({
    isDesktop: !IS_WEB,
    listen,
    invoke,
    getUserMedia: (constraints) => navigator.mediaDevices.getUserMedia(constraints),
    createRecorder: defaultRecorder,
    storeClip: storeLarkCaptureClip,
    discardClip: discardLarkCaptureClip,
    scheduleDeadline: defaultDeadline,
    now: Date.now,
    createId: () => crypto.randomUUID(),
    ...runtimeOverrides,
  });
  stateRef.current = state;
  tabsRef.current = tabs;
  runtimeRef.current = {
    isDesktop: !IS_WEB,
    listen,
    invoke,
    getUserMedia: (constraints) => navigator.mediaDevices.getUserMedia(constraints),
    createRecorder: defaultRecorder,
    storeClip: storeLarkCaptureClip,
    discardClip: discardLarkCaptureClip,
    scheduleDeadline: defaultDeadline,
    now: Date.now,
    createId: () => crypto.randomUUID(),
    ...runtimeOverrides,
  };

  const mountedRef = useRef(false);
  const streamRef = useRef<{ operation: number; stream: MediaStream } | null>(null);
  const recorderRef = useRef<{ operation: number; recorder: MediaRecorder } | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const mimeRef = useRef("audio/webm");
  const deadlineStopRef = useRef<(() => void) | null>(null);
  const pendingMediaRef = useRef(new Set<number>());
  const stopRequestedRef = useRef(new Set<number>());
  const startedRecorderOpsRef = useRef(new Set<number>());
  const completeAfterHostStopRef = useRef<((operation: number, capture: LarkCaptureLease) => void) | null>(null);
  const cancelRequestsRef = useRef(new Map<string, Promise<void>>());
  const captureStoppedRequestsRef = useRef(new Set<number>());
  const bindingByLeaseRef = useRef(new Map<string, string>());
  const sendIdByOperationRef = useRef(new Map<number, string>());
  const finalizedOperationsRef = useRef(new Set<number>());

  const transition = useCallback((action: LarkCaptureAction): LarkCaptureState => {
    const previous = stateRef.current;
    const next = larkCaptureTransition(previous, action);
    if (next !== previous) {
      stateRef.current = next;
      if (mountedRef.current) setState(next);
    }
    return next;
  }, []);

  const releaseOwner = useCallback(() => {
    releaseCaptureOwnership("lark");
  }, []);

  const cancelHostLease = useCallback((capture: LarkCaptureLease, _operation?: number) => {
    let request = cancelRequestsRef.current.get(capture.lease);
    if (!request) {
      request = runtimeRef.current.invoke<void>("local_recording_cancel", { lease: capture.lease });
      cancelRequestsRef.current.set(capture.lease, request);
      void request.then(
        () => cancelRequestsRef.current.delete(capture.lease),
        () => cancelRequestsRef.current.delete(capture.lease),
      );
    }
  }, []);

  const operationIs = useCallback((operation: number, ...phases: LarkCaptureState["phase"][]) => {
    const current = stateRef.current;
    return current.operation === operation && (phases.length === 0 || phases.includes(current.phase));
  }, []);

  const clearDeadline = useCallback(() => {
    deadlineStopRef.current?.();
    deadlineStopRef.current = null;
  }, []);

  const stopTracks = useCallback((operation: number): boolean => {
    const current = streamRef.current;
    if (!current || current.operation !== operation) return true;
    for (const track of current.stream.getTracks()) {
      if (track.readyState === "ended") continue;
      try {
        track.stop();
      } catch {
        track.addEventListener("ended", () => {
          const active = streamRef.current;
          if (active?.operation === operation
            && active.stream.getTracks().every((item) => item.readyState === "ended")) {
            transition({ type: "TRACKS_RELEASED", operation });
            const current = stateRef.current;
            if (current.operation === operation && current.phase === "stopping"
              && current.recorderStopped && current.stopIntent === "send" && !current.invalidation
              && current.capture) {
              completeAfterHostStopRef.current?.(operation, current.capture);
            }
          }
        }, { once: true });
      }
    }
    return current.stream.getTracks().every((track) => track.readyState === "ended");
  }, [transition]);

  const cleanupStream = useCallback((operation: number) => {
    if (stopTracks(operation)) {
      transition({ type: "TRACKS_RELEASED", operation });
      streamRef.current = null;
    }
  }, [stopTracks, transition]);

  const requestRecorderStop = useCallback((operation: number) => {
    const active = recorderRef.current;
    if (!active || active.operation !== operation) {
      if (!pendingMediaRef.current.has(operation)) {
        transition({ type: "RECORDER_STOPPED", operation });
        cleanupStream(operation);
      }
      return;
    }
    clearDeadline();
    cleanupStream(operation);
    if (stopRequestedRef.current.has(operation)) return;
    stopRequestedRef.current.add(operation);
    if (active.recorder.state !== "inactive") {
      try {
        active.recorder.stop();
      } catch (error) {
        transition({ type: "FAILED", operation, message: errorText(error) });
      }
    } else if (!startedRecorderOpsRef.current.has(operation)) {
      transition({ type: "RECORDER_STOPPED", operation });
      cleanupStream(operation);
    }
  }, [clearDeadline, cleanupStream, transition]);

  const cancelCapture = useCallback((message = "Voice recording cancelled.", reason?: LarkCaptureInvalidation["reason"]) => {
    const current = stateRef.current;
    const capture = current.capture;
    if (!capture || current.phase === "disabled" || current.phase === "armed" || current.phase === "error") return;
    clearDeadline();
    const next = transition({ type: "CANCEL", operation: current.operation, message, reason });
    cancelHostLease(capture, current.operation);
    if (next.phase === "error") {
      cleanupStream(current.operation);
      releaseOwner();
      return;
    }
    requestRecorderStop(current.operation);
  }, [cancelHostLease, clearDeadline, cleanupStream, releaseOwner, requestRecorderStop, transition]);

  const completeCapture = useCallback(async (
    operation: number,
    capture: LarkCaptureLease,
    blob: Blob,
    mime: string,
  ) => {
    if (finalizedOperationsRef.current.has(operation)) return;
    finalizedOperationsRef.current.add(operation);
    let clipHandle: string | undefined;
    let clipPath: string | undefined;
    let sendId: string | undefined;
    let handoffAttempted = false;
    const reconcilePossibleHandoff = async () => {
      try {
        const statuses = await runtimeRef.current.invoke<ChatSendStatus[]>("chat_send_status", { tabId: capture.tabId });
        statuses
          .filter((status) => status.tab_id === capture.tabId && status.correlationId === sendId)
          .forEach(observeDeliveryStatus);
      } catch {
        return;
      }
    };
    try {
      if (!operationIs(operation, "transcribing")) return;
      if (blob.size < MIN_VOICE_BYTES) throw new Error("Voice recording produced no usable audio.");
      const binding = bindingByLeaseRef.current.get(capture.lease);
      if (!binding || tabBinding(tabsRef.current[capture.tabId]) !== binding) {
        throw new Error("The captured recipient closed or changed before voice delivery.");
      }
      const storedClip = await runtimeRef.current.storeClip(capture.lease, blob, mime);
      clipHandle = storedClip.handle;
      clipPath = storedClip.path;
      if (!operationIs(operation, "transcribing")) {
        const staleHandle = clipHandle;
        clipHandle = undefined;
        await runtimeRef.current.discardClip(capture.lease, staleHandle).catch(() => {});
        return;
      }
      if (!clipHandle.trim() || !clipPath.trim()) throw new Error("Voice recording could not be saved.");
      sendId = runtimeRef.current.createId();
      sendIdByOperationRef.current.set(operation, sendId);
      addChatSend(capture.tabId, {
        id: sendId,
        correlationId: sendId,
        content: markVoice(clipPath),
        sentAtMs: runtimeRef.current.now(),
      });
      handoffAttempted = true;
      const outcome = await runtimeRef.current.invoke<LocalRecordingCommitOutcome>("local_recording_commit", {
        lease: capture.lease,
        clipHandle,
        clientToken: sendId,
      });
      if (outcome.kind === "tracked") {
        const { status } = outcome;
        if (status.tab_id !== capture.tabId || status.correlationId !== sendId) {
          throw new Error("Voice delivery returned a different frozen recipient.");
        }
        observeDeliveryStatus(status);
        if (operationIs(operation, "transcribing")) transition({ type: "COMMIT_SUCCEEDED", operation });
        bindingByLeaseRef.current.delete(capture.lease);
        clipHandle = undefined;
        return;
      }
      if (outcome.kind === "possibleHandoff") {
        clipHandle = undefined;
        if (operationIs(operation, "transcribing")) {
          transition({ type: "FAILED", operation, message: microphoneErrorMessage(outcome.message) });
        }
        await reconcilePossibleHandoff();
        bindingByLeaseRef.current.delete(capture.lease);
        return;
      }
      if (outcome.kind === "preHandoffFailure") {
        clipHandle = undefined;
        removeChatSend(capture.tabId, sendId);
        if (operationIs(operation, "transcribing")) {
          transition({ type: "FAILED", operation, message: microphoneErrorMessage(outcome.message) });
        }
        cancelHostLease(capture);
        bindingByLeaseRef.current.delete(capture.lease);
        return;
      }
    } catch (error) {
      const message = microphoneErrorMessage(error);
      if (handoffAttempted && sendId) {
        if (operationIs(operation, "transcribing")) {
          transition({ type: "FAILED", operation, message: "Voice delivery may have been accepted; its tracked status is being reconciled." });
        }
        await reconcilePossibleHandoff();
      } else {
        if (clipHandle) {
          const failedHandle = clipHandle;
          clipHandle = undefined;
          await runtimeRef.current.discardClip(capture.lease, failedHandle).catch(() => {});
        }
        if (operationIs(operation, "transcribing")) {
          const pendingSendId = sendIdByOperationRef.current.get(operation);
          if (pendingSendId) removeChatSend(capture.tabId, pendingSendId);
          cancelHostLease(capture);
          transition({ type: "FAILED", operation, message });
        }
      }
      bindingByLeaseRef.current.delete(capture.lease);
    } finally {
      releaseOwner();
    }
  }, [cancelHostLease, operationIs, releaseOwner, transition]);

  const completeAfterHostStop = useCallback((operation: number, capture: LarkCaptureLease) => {
    if (!operationIs(operation, "stopping") || captureStoppedRequestsRef.current.has(operation)) return;
    captureStoppedRequestsRef.current.add(operation);
    void runtimeRef.current.invoke<void>("local_recording_capture_stopped", { lease: capture.lease }).then(() => {
      const next = transition({ type: "CAPTURE_STOP_ACK", operation });
      if (next.phase === "transcribing" && next.capture) {
        const blob = new Blob(chunksRef.current, { type: mimeRef.current });
        chunksRef.current = [];
        recorderRef.current = null;
        streamRef.current = null;
        releaseOwner();
        void completeCapture(operation, capture, blob, mimeRef.current);
        return;
      }
      if (next.phase !== "stopping") {
        chunksRef.current = [];
        recorderRef.current = null;
        streamRef.current = null;
        bindingByLeaseRef.current.delete(capture.lease);
        releaseOwner();
      }
    }).catch((error) => {
      transition({ type: "FAILED", operation, message: errorText(error) });
      cancelHostLease(capture, operation);
    });
  }, [cancelHostLease, completeCapture, operationIs, releaseOwner, transition]);
  completeAfterHostStopRef.current = completeAfterHostStop;

  const finishRecorderStop = useCallback((operation: number, capture: LarkCaptureLease) => {
    const current = stateRef.current;
    if (current.operation !== operation) return;
    if (current.phase !== "stopping") {
      transition({ type: "CANCEL", operation, message: "Voice capture stopped unexpectedly." });
      cancelHostLease(capture, operation);
    }
    cleanupStream(operation);
    transition({ type: "RECORDER_STOPPED", operation });
    const released = streamRef.current == null
      || streamRef.current.stream.getTracks().every((track) => track.readyState === "ended");
    if (released) transition({ type: "TRACKS_RELEASED", operation });
    const settled = stateRef.current;
    if (settled.phase === "stopping" && settled.recorderStopped && settled.tracksReleased
      && settled.stopIntent === "send" && !settled.invalidation) {
      completeAfterHostStop(operation, capture);
      return;
    }
    if (settled.phase === "error" || settled.phase === "armed") {
      chunksRef.current = [];
      recorderRef.current = null;
      streamRef.current = null;
      bindingByLeaseRef.current.delete(capture.lease);
      releaseOwner();
    }
  }, [cancelHostLease, cleanupStream, completeAfterHostStop, releaseOwner, transition]);

  const startCapture = useCallback(async (capture: LarkCaptureLease) => {
    const before = stateRef.current;
    const started = transition({ type: "START", capture });
    if (started === before) {
      if (before.capture?.lease !== capture.lease) cancelHostLease(capture);
      return;
    }
    const operation = started.operation;
    if (!claimCaptureOwnership("lark")) {
      transition({ type: "CANCEL", operation, message: "Another voice capture is already active." });
      cancelHostLease(capture, operation);
      transition({ type: "RECORDER_STOPPED", operation });
      cleanupStream(operation);
      return;
    }
    const binding = tabBinding(tabsRef.current[capture.tabId]);
    if (!binding) {
      transition({ type: "CANCEL", operation, message: "The captured recipient is no longer open." });
      cancelHostLease(capture, operation);
      transition({ type: "RECORDER_STOPPED", operation });
      cleanupStream(operation);
      return;
    }
    bindingByLeaseRef.current.set(capture.lease, binding);
    pendingMediaRef.current.add(operation);
    try {
      const stream = await runtimeRef.current.getUserMedia(exactMicrophoneConstraints(capture.microphoneId));
      pendingMediaRef.current.delete(operation);
      if (!operationIs(operation, "starting")) {
        stream.getTracks().forEach((track) => track.stop());
        transition({ type: "RECORDER_STOPPED", operation });
        const settled = transition({ type: "TRACKS_RELEASED", operation });
        if (settled.phase !== "stopping") {
          chunksRef.current = [];
          recorderRef.current = null;
          bindingByLeaseRef.current.delete(capture.lease);
          releaseOwner();
        }
        return;
      }
      if (!hasExactLiveMicrophone(stream, capture.microphoneId)
        || stream.getTracks().some((track) => track.kind !== "audio")) {
        stream.getTracks().forEach((track) => track.stop());
        throw new Error("The browser did not return the selected live microphone.");
      }
      streamRef.current = { operation, stream };
      const mime = pickAudioMime();
      mimeRef.current = mime ?? "audio/webm";
      chunksRef.current = [];
      const recorder = runtimeRef.current.createRecorder(stream, mime);
      mimeRef.current = recorder.mimeType || mimeRef.current;
      recorderRef.current = { operation, recorder };
      const onTrackEnded = () => {
        const current = stateRef.current;
        if (current.operation !== operation || !current.capture) return;
        cancelCapture("The selected microphone became unavailable during recording.", "deviceUnavailable");
      };
      for (const track of stream.getAudioTracks()) track.addEventListener("ended", onTrackEnded, { once: true });
      recorder.ondataavailable = (event) => {
        const current = stateRef.current;
        if (current.operation !== operation || current.invalidation || current.stopIntent === "discard") return;
        if (event.data.size > 0) chunksRef.current.push(event.data);
      };
      recorder.onstart = () => {
        if (operationIs(operation, "starting") && stream.getAudioTracks().every((track) => track.readyState === "live")) {
          startedRecorderOpsRef.current.add(operation);
          transition({ type: "RECORDER_STARTED", operation });
          deadlineStopRef.current = runtimeRef.current.scheduleDeadline(
            () => cancelCapture("The two minute recording limit was reached. The clip was discarded."),
            MAX_CAPTURE_MS,
          );
          return;
        }
        if (operationIs(operation, "stopping") && recorder.state !== "inactive") requestRecorderStop(operation);
      };
      recorder.onerror = () => {
        cancelCapture("The browser recorder failed during hardware voice capture.", "deviceUnavailable");
      };
      recorder.onstop = () => finishRecorderStop(operation, capture);
      recorder.start();
      if (recorder.state === "recording" && operationIs(operation, "stopping")) requestRecorderStop(operation);
    } catch (error) {
      pendingMediaRef.current.delete(operation);
      if (operationIs(operation, "stopping")) {
        transition({ type: "RECORDER_STOPPED", operation });
        cleanupStream(operation);
        const settled = stateRef.current;
        if (settled.phase !== "stopping") {
          chunksRef.current = [];
          recorderRef.current = null;
          bindingByLeaseRef.current.delete(capture.lease);
          releaseOwner();
        }
      } else if (operationIs(operation, "starting")) {
        const message = microphoneErrorMessage(error);
        transition({ type: "CANCEL", operation, message, reason: microphoneFailureReason(error) });
        cancelHostLease(capture, operation);
        if (!recorderRef.current || recorderRef.current.recorder.state === "inactive") {
          transition({ type: "RECORDER_STOPPED", operation });
        } else {
          requestRecorderStop(operation);
        }
        streamRef.current?.stream.getTracks().forEach((track) => track.stop());
        cleanupStream(operation);
      }
    }
  }, [cancelCapture, cancelHostLease, cleanupStream, finishRecorderStop, operationIs, releaseOwner, requestRecorderStop, transition]);

  const stopCapture = useCallback((lease: string) => {
    const current = stateRef.current;
    if (current.capture?.lease !== lease || !["starting", "recording"].includes(current.phase)) return;
    const next = transition({ type: "STOP", operation: current.operation, intent: "send" });
    if (next.stopIntent !== "send") {
      const capture = current.capture;
      cancelHostLease(capture, current.operation);
    }
    requestRecorderStop(current.operation);
  }, [cancelHostLease, requestRecorderStop, transition]);

  const handleInvalidation = useCallback((event: LarkCaptureInvalidation) => {
    const current = stateRef.current;
    const capture = current.capture;
    if (!capture || capture.pluginId !== event.pluginId || capture.generation !== event.generation) return;
    const invalidated = transition({ type: "INVALIDATED", event });
    clearDeadline();
    cancelHostLease(capture);
    if (current.phase === "transcribing" || invalidated.phase !== "stopping") {
      releaseOwner();
      return;
    }
    requestRecorderStop(current.operation);
  }, [cancelHostLease, clearDeadline, releaseOwner, requestRecorderStop, transition]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      clearDeadline();
      const capture = stateRef.current.capture;
      if (capture) cancelHostLease(capture);
      const recorder = recorderRef.current?.recorder;
      if (recorder && recorder.state !== "inactive") {
        try { recorder.stop(); } catch { }
      }
      streamRef.current?.stream.getTracks().forEach((track) => track.stop());
      streamRef.current = null;
      chunksRef.current = [];
      releaseOwner();
      stateRef.current = { ...initialLarkCaptureState(), operation: stateRef.current.operation + 1 };
    };
  }, [cancelHostLease, clearDeadline, releaseOwner]);

  useEffect(() => {
    if (!runtimeRef.current.isDesktop) {
      resolveListenerReadyRef.current?.();
      return;
    }
    let disposed = false;
    const unlisten: UnlistenFn[] = [];
    void Promise.all([
      runtimeRef.current.listen<LarkRecordingStatus>("local-recording-status", ({ payload }) => {
        if (statusPayload(payload)) transition({ type: "STATUS", status: payload });
      }),
      runtimeRef.current.listen<CaptureEvent>("local-recording-capture", ({ payload }) => {
        if (!captureEvent(payload)) return;
        if (payload.kind === "start") void startCapture(payload.capture);
        else stopCapture(payload.lease);
      }),
      runtimeRef.current.listen<LarkCaptureInvalidation>("local-recording-capture-invalidated", ({ payload }) => {
        if (invalidationPayload(payload)) handleInvalidation(payload);
      }),
    ]).then((listeners) => {
      if (disposed) listeners.forEach((unlistenEvent) => unlistenEvent());
      else unlisten.push(...listeners);
      resolveListenerReadyRef.current?.();
    }).catch((error) => {
      transition({ type: "FAILED", operation: stateRef.current.operation, message: errorText(error) });
      resolveListenerReadyRef.current?.();
    });
    return () => {
      disposed = true;
      unlisten.forEach((unlistenEvent) => unlistenEvent());
      resolveListenerReadyRef.current?.();
    };
  }, [handleInvalidation, startCapture, stopCapture, transition]);

  useEffect(() => {
    const current = stateRef.current;
    const capture = current.capture;
    if (!capture || current.phase === "error") return;
    const expected = bindingByLeaseRef.current.get(capture.lease);
    if (expected && tabBinding(tabs[capture.tabId]) !== expected) {
      cancelCapture("The captured recipient closed or changed. The clip was discarded.");
    }
  }, [cancelCapture, tabs]);

  return { state, cancel: () => cancelCapture(), ready: listenerReadyRef.current };
}
