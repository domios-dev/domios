// Run: npx tsx src/components/Terminal/voice/useLocalRecordingCapture.test.tsx
import assert from "node:assert/strict";
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { Tab } from "../../../lib/types";
import type { LocalRecordingCaptureRuntime } from "./useLocalRecordingCapture";
import type { ChatSendStatus } from "../chatSendStatus";
import {
  claimCaptureOwnership,
  currentCaptureOwner,
  releaseCaptureOwnership,
  resetCaptureOwnershipForTest,
  type LocalRecordingCaptureLease,
  type LocalRecordingStatus,
} from "./localRecordingCaptureModel";

const hookWindow = new Window({ url: "http://localhost/" });
const originalGlobals = new Map<string, PropertyDescriptor | undefined>();
for (const [name, value] of Object.entries({
  window: hookWindow,
  document: hookWindow.document,
  navigator: hookWindow.navigator,
  HTMLElement: hookWindow.HTMLElement,
  HTMLButtonElement: hookWindow.HTMLButtonElement,
  Element: hookWindow.Element,
  Node: hookWindow.Node,
  Event: hookWindow.Event,
  CustomEvent: hookWindow.CustomEvent,
  MutationObserver: hookWindow.MutationObserver,
})) {
  originalGlobals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
  Object.defineProperty(globalThis, name, { configurable: true, value });
}
originalGlobals.set("IS_REACT_ACT_ENVIRONMENT", Object.getOwnPropertyDescriptor(globalThis, "IS_REACT_ACT_ENVIRONMENT"));
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });

function restoreGlobals(): void {
  for (const [name, descriptor] of originalGlobals) {
    if (descriptor) Object.defineProperty(globalThis, name, descriptor);
    else Reflect.deleteProperty(globalThis, name);
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((next, fail) => {
    resolve = next;
    reject = fail;
  });
  return { promise, resolve, reject };
}

async function flushAct() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

class FakeTrack extends EventTarget {
  readonly id = "track-exact";
  readonly kind = "audio";
  readonly label = "Selected microphone";
  readonly enabled = true;
  readonly muted = false;
  private currentState: MediaStreamTrackState = "live";
  stopCalls = 0;

  get readyState(): MediaStreamTrackState { return this.currentState; }
  getSettings(): MediaTrackSettings { return { deviceId: "mic-A" } as MediaTrackSettings; }
  stop(): void { this.stopCalls += 1; this.currentState = "ended"; }
}

class FakeStream {
  readonly track = new FakeTrack();
  getAudioTracks(): MediaStreamTrack[] { return [this.track as unknown as MediaStreamTrack]; }
  getTracks(): MediaStreamTrack[] { return [this.track as unknown as MediaStreamTrack]; }
}

class FakeRecorder {
  readonly stream: MediaStream;
  readonly mimeType = "audio/webm";
  state: RecordingState = "inactive";
  ondataavailable: ((event: BlobEvent) => void) | null = null;
  onstart: ((event: Event) => void) | null = null;
  onstop: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  startCalls = 0;
  stopCalls = 0;

  constructor(stream: MediaStream) { this.stream = stream; }
  start(): void { this.startCalls += 1; this.state = "recording"; }
  stop(): void { this.stopCalls += 1; this.state = "inactive"; }
  emitStart(): void { this.onstart?.(new Event("start")); }
  emitData(blob = new Blob([new Uint8Array(6 * 1024)], { type: this.mimeType })): void {
    this.ondataavailable?.({ data: blob } as BlobEvent);
  }
  emitStop(): void { this.onstop?.(new Event("stop")); }
}

function tab(id: string, session: string): Tab {
  return {
    id,
    title: id,
    pty_id: null,
    tab_type: "Terminal",
    cwd: null,
    backing: { kind: "chat_backend", plugin_id: "backend", session_id: session } as never,
    agent_session_id: session,
    session_binding_nonce: `nonce-${session}`,
  };
}

function recordingStatus(generation: number): LocalRecordingStatus {
  return { pluginId: "lark", granted: true, enabled: true, available: true, generation };
}

function lease(leaseId: string, generation: number): LocalRecordingCaptureLease {
  return { lease: leaseId, pluginId: "lark", tabId: "A", microphoneId: "mic-A", generation };
}

async function main() {
  const bridge = await import("../../../lib/bridge");
  const { useLocalRecordingCapture } = await import("./useLocalRecordingCapture");
  const { useComposerVoice } = await import("./useComposerVoice");
  const { LocalRecordingCaptureStatus } = await import("./LocalRecordingCaptureStatus");
  const { readChatSends, forgetChatSendsInMemory } = await import("../chatSendStore");
  resetCaptureOwnershipForTest();
  const listeners = new Map<string, (event: { payload: unknown }) => void>();
  const invocations: Array<{ command: string; args: Record<string, unknown> }> = [];
  const mediaCalls: MediaStreamConstraints[] = [];
  const streams: FakeStream[] = [];
  const recorders: FakeRecorder[] = [];
  const uploads: Array<{ blob: Blob; mime: string }> = [];
  const storeCompletions: Array<ReturnType<typeof deferred<{ handle: string; path: string }>>> = [];
  const discardedClips: Array<{ lease: string; handle: string }> = [];
  const acceptedStatuses: unknown[] = [];
  const reconciledStatuses: ChatSendStatus[] = [];
  const hostAcceptedStatuses = new Map<string, ChatSendStatus>();
  const captureStopAcks: Array<ReturnType<typeof deferred<void>>> = [];
  const commitCompletions: Array<ReturnType<typeof deferred<{
    kind: "tracked";
    status: {
      send_id: string;
      tab_id: string;
      state: "delivered" | "written";
      error: null;
      ledger_id: string;
      sequence: number;
      retained_from: number;
      correlationId: string;
    };
  } | { kind: "preHandoffFailure"; message: string } | { kind: "possibleHandoff"; message: string }>>> = [];
  let deferNextStore = false;
  let deferredStatus: ReturnType<typeof deferred<LocalRecordingStatus>> | null = null;
  let hostStatus = { ...recordingStatus(1), enabled: false, available: false };
  const runtime: Partial<LocalRecordingCaptureRuntime> = {
    isDesktop: true,
    getUserMedia: async (constraints) => {
      mediaCalls.push(constraints);
      const stream = new FakeStream();
      streams.push(stream);
      return stream as unknown as MediaStream;
    },
    createRecorder: (stream) => {
      const recorder = new FakeRecorder(stream) as unknown as MediaRecorder;
      recorders.push(recorder as unknown as FakeRecorder);
      return recorder;
    },
    storeClip: async (_leaseId, blob, mime) => {
      uploads.push({ blob, mime });
      if (deferNextStore) {
        deferNextStore = false;
        const completion = deferred<{ handle: string; path: string }>();
        storeCompletions.push(completion);
        return completion.promise;
      }
      return { handle: `capture-clip-${uploads.length}`, path: `C:/temp/voice-${uploads.length}.webm` };
    },
    discardClip: async (leaseId, handle) => { discardedClips.push({ lease: leaseId, handle }); },
    now: () => 123456,
    createId: () => `voice-send-${acceptedStatuses.length + invocations.filter((item) => item.command === "local_recording_commit").length + 1}`,
  };
  bridge.__setListenForTest(async (event, handler) => {
    listeners.set(event, handler as (event: { payload: unknown }) => void);
    return () => listeners.delete(event);
  });
  bridge.__setInvokeForTest(async <T,>(command: string, args: Record<string, unknown> = {}) => {
    invocations.push({ command, args });
    if (command === "local_recording_status") return (deferredStatus ? deferredStatus.promise : hostStatus) as T;
    if (command === "local_recording_cancel") {
      return undefined as T;
    }
    if (command === "local_recording_capture_stopped") {
      const ack = deferred<void>();
      captureStopAcks.push(ack);
      return ack.promise as T;
    }
    if (command === "local_recording_commit") {
      const completion = deferred<{
        kind: "tracked";
        status: {
          send_id: string;
          tab_id: string;
          state: "delivered" | "written";
          error: null;
          ledger_id: string;
          sequence: number;
          retained_from: number;
          correlationId: string;
        };
      } | { kind: "preHandoffFailure"; message: string } | { kind: "possibleHandoff"; message: string }>();
      commitCompletions.push(completion);
      return completion.promise.then((outcome) => {
        if (outcome.kind === "tracked") acceptedStatuses.push(outcome.status);
        return outcome as T;
      });
    }
    if (command === "chat_send_status") return reconciledStatuses as T;
    throw new Error(`Unexpected command ${command}`);
  });

  let latest: ReturnType<typeof useLocalRecordingCapture> | null = null;
  let composer: ReturnType<typeof useComposerVoice> | null = null;
  const current = () => {
    assert(latest, "hook rendered");
    return latest;
  };
  function Harness({ tabs }: { tabs: Record<string, Tab> }) {
    composer = useComposerVoice({
      tabId: "A", hasText: false, onSend: () => {}, onAttach: () => {},
      onSendWithAttachments: () => { throw new Error("Hardware completion must retain frozen lease admission."); },
      onError: () => {},
    });
    latest = useLocalRecordingCapture({ tabs, runtime });
    return React.createElement(LocalRecordingCaptureStatus, { state: current().state });
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  let currentTabs = { A: tab("A", "session-A"), B: tab("B", "session-B") };

  const emit = (event: string, payload: unknown) => {
    if (event === "local-recording-status") hostStatus = payload as typeof hostStatus;
    listeners.get(event)?.({ payload });
  };
  const invokeCount = (command: string) => invocations.filter((item) => item.command === command).length;
  const statusRow = () => host.querySelector('[role="status"]')?.textContent ?? "";
  const alertText = () => host.querySelector('[role="alert"]')?.textContent ?? "";
  const render = async () => act(async () => { root.render(React.createElement(Harness, { tabs: currentTabs })); });
  const startCapture = async (capture: LocalRecordingCaptureLease) => {
    hostStatus = { ...recordingStatus(capture.generation), pluginId: capture.pluginId };
    await act(async () => emit("local-recording-capture", { kind: "start", capture }));
    await flushAct();
  };
  const emitRecorderStarted = async (index: number) => act(async () => recorders[index].emitStart());
  const stopByHost = async (leaseId: string) => act(async () => emit("local-recording-capture", { kind: "stop", lease: leaseId }));
  const stopRecorder = async (index: number) => {
    await act(async () => {
      recorders[index].emitData();
      recorders[index].emitStop();
    });
    await flushAct();
  };

  try {
    await render();
    await flushAct();
    assert.equal(listeners.size, 4, "desktop status, capture, invalidation and receiver binding events are installed");
    emit("local-recording-capture", { kind: "start", capture: lease("disabled-start", 1) });
    await flushAct();
    assert.equal(mediaCalls.length, 0, "disabled START cannot acquire audio");

    await act(async () => emit("local-recording-status", recordingStatus(12)));
    assert.equal(current().state.phase, "armed");
    assert.equal(mediaCalls.length, 0, "armed state captures nothing");
    assert.equal(claimCaptureOwnership("manual"), true);
    await act(async () => emit("local-recording-capture-invalidated", {
      pluginId: "lark", generation: 11, reason: "deviceRemoved",
    }));
    assert.equal(currentCaptureOwner(), "manual", "stale foreign-generation invalidation leaves manual capture owned");
    releaseCaptureOwnership("manual");
    await startCapture(lease("lease-valid", 12));
    assert.deepEqual(mediaCalls[0], { audio: { deviceId: { exact: "mic-A" } } });
    assert.equal(current().state.phase, "starting", "MediaRecorder start acknowledgement is still pending");
    assert.equal(recorders[0].startCalls, 1);
    await emitRecorderStarted(0);
    assert.equal(current().state.phase, "recording");
    assert.match(statusRow(), /recording is active/i);

    currentTabs = { ...currentTabs, A: tab("A", "session-A"), B: tab("B", "session-B-focused") };
    await render();
    await flushAct();
    await stopByHost("lease-valid");
    assert.equal(current().state.phase, "stopping");
    assert.equal(streams[0].track.readyState, "ended", "selected stream releases immediately on STOP request");
    assert.equal(streams[0].track.stopCalls, 1, "one active clip releases its microphone exactly once");
    assert.equal(invokeCount("local_recording_capture_stopped"), 0, "indicator waits for MediaRecorder stop event");
    await stopRecorder(0);
    assert.equal(current().state.phase, "stopping", "recorder stop and track release still wait for capture lease state acknowledgement");
    assert.equal(invokeCount("local_recording_capture_stopped"), 1);
    assert.equal(invokeCount("local_recording_commit"), 0);
    captureStopAcks[0].resolve(undefined);
    await flushAct();
    assert.equal(current().state.phase, "transcribing");
    assert.equal(invokeCount("local_recording_commit"), 1);
    const sendsA = readChatSends("A");
    assert.equal(sendsA.length, 1, "one optimistic owner row is created in frozen A");
    assert.match(sendsA[0].content, /voice/i);
    assert.equal(readChatSends("B").length, 0, "focus changes do not retarget the clip");
    const validToken = String(invocations.find((item) => item.command === "local_recording_commit")?.args.clientToken);
    commitCompletions[0].resolve({
      kind: "tracked",
      status: {
        send_id: "send-valid",
        tab_id: "A",
        state: "delivered",
        error: null,
        ledger_id: "ledger-valid",
        sequence: 1,
        retained_from: 1,
        correlationId: validToken,
      },
    });
    await flushAct();
    assert.equal(acceptedStatuses.length, 1, "one returned owner status reconciles the optimistic row");
    assert.equal(discardedClips.length, 0, "tracked success retains the capture clip under ordinary attachment policy");
    assert.equal(current().state.phase, "armed");

    await act(async () => emit("local-recording-status", recordingStatus(13)));
    await startCapture(lease("lease-access-loss", 13));
    await emitRecorderStarted(1);
    await stopByHost("lease-access-loss");
    await stopRecorder(1);
    assert.equal(current().state.phase, "stopping");
    captureStopAcks[1].resolve(undefined);
    await flushAct();
    assert.equal(current().state.phase, "transcribing");
    assert.equal(invokeCount("local_recording_commit"), 2);
    const staleToken = String(invocations.filter((item) => item.command === "local_recording_commit")[1].args.clientToken);
    const admittedStatus = {
      send_id: "send-stale",
      tab_id: "A",
      state: "delivered" as const,
      error: null,
      ledger_id: "ledger-stale",
      sequence: 2,
      retained_from: 1,
      correlationId: staleToken,
    };
    hostAcceptedStatuses.set(staleToken, admittedStatus);
    assert.equal(hostAcceptedStatuses.get(staleToken), admittedStatus, "host admission is recorded before response delivery");
    await act(async () => emit("local-recording-capture-invalidated", {
      pluginId: "lark", generation: 13, reason: "accessLost",
    }));
    assert.equal(current().state.phase, "error");
    assert.match(alertText(), /access was lost/i);
    commitCompletions[1].resolve({
      kind: "tracked",
      status: admittedStatus,
    });
    await flushAct();
    assert.equal(acceptedStatuses.length, 2, "late tracked status is reconciled after invalidation");
    const reconciled = readChatSends("A").find((send) => send.correlationId === staleToken);
    assert(reconciled, "accepted owner row remains after invalidation");
    assert.equal(reconciled.status?.state, "delivered", "late status updates the same retained row");
    assert.equal(invokeCount("local_recording_commit"), 2, "late status reconciliation never retries the handoff");

    await act(async () => emit("local-recording-status", recordingStatus(14)));
    await startCapture(lease("lease-removal", 14));
    await emitRecorderStarted(2);
    await act(async () => emit("local-recording-capture-invalidated", {
      pluginId: "lark", generation: 13, reason: "deviceRemoved",
    }));
    assert.equal(current().state.phase, "recording", "stale old generation cannot stop a newly armed capture");
    assert.equal(streams[2].track.readyState, "live");
    await act(async () => emit("local-recording-capture-invalidated", {
      pluginId: "lark", generation: 14, reason: "deviceRemoved",
    }));
    assert.equal(current().state.phase, "stopping");
    assert.equal(streams[2].track.readyState, "ended");
    assert.equal(streams[2].track.stopCalls, 1, "device removal releases its microphone exactly once");
    await stopRecorder(2);
    assert.equal(current().state.phase, "error");
    assert.match(alertText(), /device was removed/i);
    assert.equal(invokeCount("local_recording_commit"), 2, "removal discards without uploading or committing");

    await act(async () => emit("local-recording-status", recordingStatus(15)));
    await startCapture(lease("lease-rebind", 15));
    await emitRecorderStarted(3);
    currentTabs = { ...currentTabs, A: tab("A", "session-A-rebound") };
    await render();
    assert.equal(current().state.phase, "stopping", "rebinding the frozen recipient cancels the active capture");
    assert.equal(streams[3].track.readyState, "ended");
    await stopRecorder(3);
    assert.equal(current().state.phase, "error");
    assert.match(alertText(), /recipient/i);
    assert.equal(invokeCount("local_recording_commit"), 2, "rebound A receives no clip");

    await act(async () => emit("local-recording-status", recordingStatus(16)));
    assert.equal(claimCaptureOwnership("manual"), true);
    await startCapture(lease("lease-manual-race", 16));
    assert.equal(mediaCalls.length, 4, "simultaneous manual and hardware starts obtain one stream total");
    assert.equal(invocations.filter((item) => item.command === "local_recording_cancel" && item.args.lease === "lease-manual-race").length, 1, "busy hardware lease is canceled once");
    assert.match(alertText(), /another voice capture/i);
    await act(async () => releaseCaptureOwnership("manual"));

    await act(async () => emit("local-recording-status", recordingStatus(17)));
    await startCapture(lease("lease-composer-cancel", 17));
    await emitRecorderStarted(4);
    await act(async () => composer!.cancelVoice());
    assert.equal(current().state.phase, "stopping");
    assert.equal(streams[4].track.readyState, "ended", "composer cancel immediately releases the selected stream");
    assert.equal(streams[4].track.stopCalls, 1, "composer cancel releases its microphone exactly once");
    await stopRecorder(4);
    assert.equal(current().state.phase, "error");
    assert.match(alertText(), /cancelled/i);
    assert.equal(invokeCount("local_recording_commit"), 2, "composer cancel discards and does not retry");

    hostStatus = recordingStatus(18);
    await act(async () => emit("contributions-changed", { kind: "receiver_target" }));
    await flushAct();
    assert.equal(current().state.phase, "armed", "rebinding refreshes authoritative readiness after cancellation");
    await startCapture(lease("lease-typed-failure", 18));
    await emitRecorderStarted(5);
    await stopByHost("lease-typed-failure");
    await stopRecorder(5);
    assert.equal(current().state.phase, "stopping");
    captureStopAcks[2].resolve(undefined);
    await flushAct();
    assert.equal(invokeCount("local_recording_commit"), 3);
    const failedToken = String(invocations.filter((item) => item.command === "local_recording_commit")[2].args.clientToken);
    commitCompletions[2].resolve({
      kind: "preHandoffFailure",
      message: "recording_transcription_failed: transcriber unavailable",
    });
    await flushAct();
    assert.equal(current().state.phase, "error");
    assert.equal(readChatSends("A").some((send) => send.correlationId === failedToken), false);
    assert.equal(acceptedStatuses.length, 2, "typed transcription failure removes its optimistic row without another handoff");

    await act(async () => emit("local-recording-status", recordingStatus(19)));
    await startCapture(lease("lease-ui-cancel", 19));
    await emitRecorderStarted(6);
    assert.equal(host.querySelector("button"), null, "hardware status adds no separate recording popup or cancel control");
    await act(async () => composer!.cancelVoice());
    assert.equal(current().state.phase, "stopping");
    await stopRecorder(6);
    assert.equal(current().state.phase, "error");
    assert.match(alertText(), /cancelled/i);

    await act(async () => emit("local-recording-status", recordingStatus(20)));
    deferNextStore = true;
    await startCapture(lease("lease-late-upload", 20));
    await emitRecorderStarted(7);
    await stopByHost("lease-late-upload");
    await stopRecorder(7);
    captureStopAcks[3].resolve(undefined);
    await flushAct();
    assert.equal(current().state.phase, "transcribing");
    assert.equal(storeCompletions.length, 1, "capture-owned save is still pending");
    const commitCountBeforeLateSave = invokeCount("local_recording_commit");
    await act(async () => current().cancel());
    storeCompletions[0].resolve({ handle: "capture-clip-late", path: "C:/temp/voice-late.webm" });
    await flushAct();
    assert.deepEqual(discardedClips, [{ lease: "lease-late-upload", handle: "capture-clip-late" }]);
    assert.equal(invokeCount("local_recording_commit"), commitCountBeforeLateSave, "late persisted upload is discarded without handoff");
    assert.equal(acceptedStatuses.length, 2);
    assert.equal(uploads.length, 4, "four stopped clips reached the capture-owned save seam");

    await act(async () => emit("local-recording-status", recordingStatus(21)));
    await startCapture(lease("lease-uncertain-handoff", 21));
    await emitRecorderStarted(8);
    await stopByHost("lease-uncertain-handoff");
    await stopRecorder(8);
    captureStopAcks[4].resolve(undefined);
    await flushAct();
    const uncertainToken = String(invocations.filter((item) => item.command === "local_recording_commit")[3].args.clientToken);
    reconciledStatuses.push({
      send_id: "send-uncertain",
      tab_id: "A",
      state: "written",
      error: null,
      ledger_id: "ledger-uncertain",
      sequence: 3,
      retained_from: 1,
      correlationId: uncertainToken,
    });
    commitCompletions[3].resolve({
      kind: "possibleHandoff",
      message: "admission_unknown: owner admission may have completed",
    });
    await flushAct();
    const uncertainSend = readChatSends("A").find((send) => send.correlationId === uncertainToken);
    assert.equal(uncertainSend?.status?.state, "written", "possible handoff reconciles the exact ordinary owner status");
    assert.equal(discardedClips.length, 1, "possible handoff keeps its clip; only the cancelled stale upload was discarded");
    assert.equal(invokeCount("local_recording_commit"), 4, "uncertain admission is never replayed");
    assert.equal(invokeCount("chat_send_status"), 1, "possible handoff queries tracked status once");
    await act(async () => emit("local-recording-status", recordingStatus(22)));
    await startCapture(lease("lease-before-start", 22));
    await act(async () => current().cancel());
    await stopRecorder(9);
    assert.equal(current().state.phase, "error", "stop before start acknowledgement settles startup");
    assert.equal(currentCaptureOwner(), null);
    assert.equal(streams[9].track.stopCalls, 1);

    await act(async () => emit("local-recording-status", recordingStatus(23)));
    await startCapture(lease("lease-recorder-error", 23));
    await emitRecorderStarted(10);
    recorders[10].state = "inactive";
    await act(async () => recorders[10].onerror?.(new Event("error")));
    assert.equal(currentCaptureOwner(), "hardware", "inactive error still waits for the actual stop event");
    await stopRecorder(10);
    assert.equal(current().state.phase, "error", "active recorder failure cancels through the same controller");
    assert.equal(currentCaptureOwner(), null);
    assert.equal(streams[10].track.stopCalls, 1);
    assert.equal(invokeCount("local_recording_commit"), 4);

    await act(async () => emit("local-recording-status", recordingStatus(24)));
    await startCapture(lease("lease-composer-stop", 24));
    await emitRecorderStarted(11);
    assert(composer);
    await act(async () => composer!.stopToPreview());
    assert.equal(recorders[11].stopCalls, 1, "ordinary composer stop controls the hardware-started recorder");
    await act(async () => current().cancel());
    await stopRecorder(11);
    assert.equal(currentCaptureOwner(), null);
    assert.equal(invokeCount("local_recording_commit"), 4);
    hostStatus = recordingStatus(24);
    await act(async () => root.render(null));
    await render();
    await flushAct();
    await startCapture({ ...lease("foreign-plugin-start", 24), pluginId: "other-recording-plugin" });
    assert.equal(current().state.phase, "starting", "a declared different plugin starts after frontend remount without identity-specific boot assumptions");
    await act(async () => current().cancel());
    await stopRecorder(12);
    assert.equal(currentCaptureOwner(), null);
    const streamCount = streams.length;
    for (const cancellation of ["stop", "invalidation"] as const) {
      await act(async () => emit("local-recording-status", recordingStatus(25)));
      const pending = deferred<LocalRecordingStatus>();
      deferredStatus = pending;
      await startCapture(lease(`lease-before-status-${cancellation}`, 25));
      assert.equal(streams.length, streamCount, "pending authority lookup acquires no microphone");
      if (cancellation === "stop") await stopByHost(`lease-before-status-${cancellation}`);
      else await act(async () => emit("local-recording-capture-invalidated", {pluginId:"lark", generation:25, reason:"deviceRemoved"}));
      deferredStatus = null;
      pending.resolve(recordingStatus(25));
      await flushAct();
      assert.equal(streams.length, streamCount, "STOP or invalidation before lookup completion prevents late capture");
      assert.equal(currentCaptureOwner(), null);
    }
    console.log("useLocalRecordingCapture: 18 wiring fixtures passed");
  } finally {
    await act(async () => root.unmount());
    hookWindow.happyDOM.abort();
    bridge.__setInvokeForTest(null);
    bridge.__setListenForTest(null);
    forgetChatSendsInMemory();
    resetCaptureOwnershipForTest();
    restoreGlobals();
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
