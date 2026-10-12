import { useCallback, useEffect, useRef, useState } from "react";
import { markVoice } from "@codeterm/chat-core";
import type { Tab } from "../../../lib/types";
import { invoke, listen, type UnlistenFn } from "../../../lib/bridge";
import { IS_WEB } from "../../../lib/webApi";
import { addChatSend, observeDeliveryStatus, removeChatSend } from "../chatSendStore";
import type { ChatSendStatus } from "../chatSendStatus";
import {
  MIN_VOICE_BYTES,
  createVoiceRecorder,
  microphoneErrorMessage,
  storeLarkCaptureClip,
  discardLarkCaptureClip,
} from "./audio";
import {
  claimCaptureOwnership,
  initialLarkCaptureState,
  larkCaptureTransition,
  releaseCaptureOwnership,
  type LarkCaptureAction,
  type LarkCaptureInvalidation,
  type LarkCaptureLease,
  type LarkCaptureState,
  type LarkRecordingStatus,
} from "./larkCaptureModel";

import { composerRecording, type ComposerRecording, type RecordedVoiceClip } from "./composerRecording";

type CaptureEvent =
  | { kind: "start"; capture: LarkCaptureLease }
  | { kind: "stop"; lease: string };

export interface LarkCaptureRuntime {
  isDesktop: boolean;
  listen: <T>(event: string, handler: (event: { payload: T }) => void) => Promise<UnlistenFn>;
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  getUserMedia: (constraints: MediaStreamConstraints) => Promise<MediaStream>;
  createRecorder: (stream: MediaStream, mime: string | undefined) => MediaRecorder;
  storeClip: (lease: string, blob: Blob, mime: string) => Promise<{ handle: string; path: string }>;
  discardClip: (lease: string, clipHandle: string) => Promise<void>;
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
  const [setupPluginId, setSetupPluginId] = useState<string | null>(null);
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
    createRecorder: createVoiceRecorder,
    storeClip: storeLarkCaptureClip,
    discardClip: discardLarkCaptureClip,
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
    createRecorder: createVoiceRecorder,
    storeClip: storeLarkCaptureClip,
    discardClip: discardLarkCaptureClip,
    now: Date.now,
    createId: () => crypto.randomUUID(),
    ...runtimeOverrides,
  };

  const mountedRef = useRef(false);
  const recorderRef = useRef<{ operation: number; recorder: ComposerRecording } | null>(null);
  const clipsRef = useRef(new Map<number, RecordedVoiceClip | null>());
  const pendingMediaRef = useRef(new Set<number>());
  const startupRequestsRef = useRef(new Map<number, Promise<boolean>>());
  const stopRequestedRef = useRef(new Set<number>());
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

  const requestRecorderStop = useCallback((operation: number) => {
    if (pendingMediaRef.current.has(operation)) {
      void recorderRef.current?.recorder.stop(true);
      return;
    }
    if (stopRequestedRef.current.has(operation)) return;
    stopRequestedRef.current.add(operation);
    const active = recorderRef.current;
    const discard = stateRef.current.stopIntent !== "send" || !!stateRef.current.invalidation;
    const stopped = active?.operation === operation ? active.recorder.stop(discard) : Promise.resolve(null);
    void stopped.then((clip) => {
      if (stateRef.current.operation !== operation) return;
      clipsRef.current.set(operation, clip);
      transition({ type: "RECORDER_STOPPED", operation });
      transition({ type: "TRACKS_RELEASED", operation });
      const current = stateRef.current;
      if (current.phase === "stopping" && current.stopIntent === "send" && !current.invalidation && current.capture) {
        completeAfterHostStopRef.current?.(operation, current.capture);
      } else {
        clipsRef.current.delete(operation);
        recorderRef.current = null;
        releaseOwner();
      }
    }).catch((error) => {
      transition({ type: "FAILED", operation, message: errorText(error) });
    });
  }, [releaseOwner, transition]);

  const cancelCapture = useCallback((message = "Voice recording cancelled.", reason?: LarkCaptureInvalidation["reason"]) => {
    const current = stateRef.current;
    const capture = current.capture;
    if (!capture || current.phase === "disabled" || current.phase === "armed" || current.phase === "error") return;
    const next = transition({ type: "CANCEL", operation: current.operation, message, reason });
    cancelHostLease(capture, current.operation);
    if (next.phase === "error") {
      requestRecorderStop(current.operation);
      return;
    }
    requestRecorderStop(current.operation);
  }, [cancelHostLease, requestRecorderStop, transition]);

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
        const clip = clipsRef.current.get(operation);
        clipsRef.current.delete(operation);
        recorderRef.current = null;
        releaseOwner();
        if (clip) void completeCapture(operation, capture, clip.blob, clip.mime);
        else {
          cancelHostLease(capture);
          transition({ type: "FAILED", operation, message: "Voice recording produced no usable audio." });
        }
        return;
      }
      if (next.phase !== "stopping") {
        clipsRef.current.delete(operation);
        recorderRef.current = null;
        bindingByLeaseRef.current.delete(capture.lease);
        releaseOwner();
      }
    }).catch((error) => {
      transition({ type: "FAILED", operation, message: errorText(error) });
      cancelHostLease(capture, operation);
    });
  }, [cancelHostLease, completeCapture, operationIs, releaseOwner, transition]);
  completeAfterHostStopRef.current = completeAfterHostStop;

  const startCapture = useCallback(async (capture: LarkCaptureLease) => {
    const before = stateRef.current;
    const started = transition({ type: "START", capture });
    if (started === before) {
      if (before.capture?.lease !== capture.lease) cancelHostLease(capture);
      return;
    }
    const operation = started.operation;
    const recorder = composerRecording(capture.tabId);
    const binding = tabBinding(tabsRef.current[capture.tabId]);
    if (!recorder || !binding || !claimCaptureOwnership("lark")) {
      transition({ type: "CANCEL", operation, message: "The recipient recorder is unavailable or another voice capture is active." });
      cancelHostLease(capture);
      transition({ type: "RECORDER_STOPPED", operation });
      transition({ type: "TRACKS_RELEASED", operation });
      return;
    }
    bindingByLeaseRef.current.set(capture.lease, binding);
    recorderRef.current = { operation, recorder };
    pendingMediaRef.current.add(operation);
    const cancel = () => cancelCapture();
    const stop = () => {
      transition({ type: "STOP", operation, intent: "send" });
      requestRecorderStop(operation);
    };
    const runtime = {
      getUserMedia: async (constraints: MediaStreamConstraints) => {
        const stream = await runtimeRef.current.getUserMedia(constraints);
        for (const track of stream.getAudioTracks()) {
          track.addEventListener("ended", () => cancelCapture("The selected microphone became unavailable.", "deviceUnavailable"), { once: true });
        }
        return stream;
      },
      createRecorder: runtimeRef.current.createRecorder,
    };
    try {
      const startup = recorder.start(capture.microphoneId, runtime, cancel, stop);
      startupRequestsRef.current.set(operation, startup);
      const ready = await startup;
      startupRequestsRef.current.delete(operation);
      pendingMediaRef.current.delete(operation);
      if (!ready) {
        transition({ type: "CANCEL", operation, message: "The existing voice recorder could not start." });
        cancelHostLease(capture);
        requestRecorderStop(operation);
        return;
      }
      if (!operationIs(operation, "starting")) {
        requestRecorderStop(operation);
        return;
      }
      transition({ type: "RECORDER_STARTED", operation });
    } catch (error) {
      startupRequestsRef.current.delete(operation);
      pendingMediaRef.current.delete(operation);
      transition({ type: "CANCEL", operation, message: microphoneErrorMessage(error), reason: microphoneFailureReason(error) });
      cancelHostLease(capture);
      requestRecorderStop(operation);
    }
  }, [cancelCapture, cancelHostLease, operationIs, requestRecorderStop, transition]);

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
    cancelHostLease(capture);
    if (current.phase === "transcribing" || invalidated.phase !== "stopping") {
      releaseOwner();
      return;
    }
    requestRecorderStop(current.operation);
  }, [cancelHostLease, releaseOwner, requestRecorderStop, transition]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      const capture = stateRef.current.capture;
      if (capture) cancelHostLease(capture);
      const recorder = recorderRef.current?.recorder;
      const startup = startupRequestsRef.current.get(stateRef.current.operation);
      if (recorder) void Promise.all([startup?.catch(() => false), recorder.stop(true)]).finally(releaseOwner);
      else releaseOwner();
      clipsRef.current.clear();
      stateRef.current = { ...initialLarkCaptureState(), operation: stateRef.current.operation + 1 };
    };
  }, [cancelHostLease, releaseOwner]);

  useEffect(() => {
    if (!runtimeRef.current.isDesktop) {
      resolveListenerReadyRef.current?.();
      return;
    }
    let disposed = false;
    const unlisten: UnlistenFn[] = [];
    void Promise.all([
      runtimeRef.current.listen<LarkRecordingStatus>("local-recording-status", ({ payload }) => {
        if (!statusPayload(payload)) return;
        transition({ type: "STATUS", status: payload });
        if (payload.granted && payload.enabled && payload.devicesSelected === false) setSetupPluginId(payload.pluginId);
        else if (payload.devicesSelected || !payload.enabled) setSetupPluginId(null);
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

  return { state, cancel: () => cancelCapture(), ready: listenerReadyRef.current,
    setupPluginId, closeSetup: () => setSetupPluginId(null) };
}
