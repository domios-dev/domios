// Run: npx tsx src/components/Terminal/voice/useLarkCapture.test.tsx
import assert from "node:assert/strict";
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { Tab } from "../../../lib/types";
import type { LarkCaptureRuntime } from "./useLarkCapture";
import {
  claimCaptureOwnership,
  releaseCaptureOwnership,
  resetCaptureOwnershipForTest,
  type LarkCaptureLease,
  type LarkRecordingStatus,
} from "./larkCaptureModel";

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

function recordingStatus(generation: number): LarkRecordingStatus {
  return { pluginId: "lark", granted: true, enabled: true, available: true, generation };
}

function lease(leaseId: string, generation: number): LarkCaptureLease {
  return { lease: leaseId, pluginId: "lark", tabId: "A", microphoneId: "mic-A", generation };
}

async function main() {
  const bridge = await import("../../../lib/bridge");
  const { useLarkCapture } = await import("./useLarkCapture");
  const { LarkCaptureStatus } = await import("./LarkCaptureStatus");
  const { readChatSends, forgetChatSendsInMemory } = await import("../chatSendStore");
  resetCaptureOwnershipForTest();
  const listeners = new Map<string, (event: { payload: unknown }) => void>();
  const invocations: Array<{ command: string; args: Record<string, unknown> }> = [];
  const mediaCalls: MediaStreamConstraints[] = [];
  const streams: FakeStream[] = [];
  const recorders: FakeRecorder[] = [];
  const deadlines: Array<{ callback: () => void; ms: number; cancelled: boolean }> = [];
  const uploads: Array<{ blob: Blob; mime: string }> = [];
  const acceptedStatuses: unknown[] = [];
  const nativeStopAcks: Array<ReturnType<typeof deferred<void>>> = [];
  const commitCompletions: Array<ReturnType<typeof deferred<{
    send_id: string;
    tab_id: string;
    state: "delivered";
    error: null;
    ledger_id: string;
    sequence: number;
    retained_from: number;
    correlationId: string;
  }>>> = [];
  const cancelledLeases = new Set<string>();
  const runtime: Partial<LarkCaptureRuntime> = {
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
    upload: async (blob, mime) => {
      uploads.push({ blob, mime });
      return [`C:/temp/voice-${uploads.length}.webm`];
    },
    scheduleDeadline: (callback, ms) => {
      const deadline = { callback, ms, cancelled: false };
      deadlines.push(deadline);
      return () => { deadline.cancelled = true; };
    },
    now: () => 123456,
    createId: () => `voice-send-${acceptedStatuses.length + invocations.filter((item) => item.command === "local_recording_commit").length + 1}`,
  };
  bridge.__setListenForTest(async (event, handler) => {
    listeners.set(event, handler as (event: { payload: unknown }) => void);
    return () => listeners.delete(event);
  });
  bridge.__setInvokeForTest(async <T,>(command: string, args: Record<string, unknown> = {}) => {
    invocations.push({ command, args });
    if (command === "local_recording_cancel") {
      cancelledLeases.add(String(args.lease));
      return undefined as T;
    }
    if (command === "local_recording_capture_stopped") {
      const ack = deferred<void>();
      nativeStopAcks.push(ack);
      return ack.promise as T;
    }
    if (command === "local_recording_commit") {
      const completion = deferred<{
        send_id: string;
        tab_id: string;
        state: "delivered";
        error: null;
        ledger_id: string;
        sequence: number;
        retained_from: number;
        correlationId: string;
      }>();
      commitCompletions.push(completion);
      return completion.promise.then((status) => {
        if (cancelledLeases.has(String(args.lease))) throw new Error("capture_lease_stale");
        acceptedStatuses.push(status);
        return status as T;
      });
    }
    throw new Error(`Unexpected command ${command}`);
  });

  let latest: ReturnType<typeof useLarkCapture> | null = null;
  const current = () => {
    assert(latest, "hook rendered");
    return latest;
  };
  function Harness({ tabs }: { tabs: Record<string, Tab> }) {
    latest = useLarkCapture({ tabs, runtime });
    return React.createElement(LarkCaptureStatus, { state: current().state, onCancel: current().cancel });
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  let currentTabs = { A: tab("A", "session-A"), B: tab("B", "session-B") };

  const emit = (event: string, payload: unknown) => listeners.get(event)?.({ payload });
  const invokeCount = (command: string) => invocations.filter((item) => item.command === command).length;
  const statusRow = () => host.querySelector('[role="status"]')?.textContent ?? "";
  const alertText = () => host.querySelector('[role="alert"]')?.textContent ?? "";
  const render = async () => act(async () => { root.render(React.createElement(Harness, { tabs: currentTabs })); });
  const startCapture = async (capture: LarkCaptureLease) => {
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
    assert.equal(listeners.size, 3, "desktop status, capture, and invalidation events are installed");
    emit("local-recording-capture", { kind: "start", capture: lease("disabled-start", 1) });
    await flushAct();
    assert.equal(mediaCalls.length, 0, "disabled START cannot acquire audio");

    await act(async () => emit("local-recording-status", recordingStatus(12)));
    assert.equal(current().state.phase, "armed");
    assert.equal(mediaCalls.length, 0, "armed state captures nothing");
    await startCapture(lease("lease-valid", 12));
    assert.deepEqual(mediaCalls[0], { audio: { deviceId: { exact: "mic-A" } } });
    assert.equal(current().state.phase, "starting", "MediaRecorder start acknowledgement is still pending");
    assert.equal(recorders[0].startCalls, 1);
    await emitRecorderStarted(0);
    assert.equal(current().state.phase, "recording");
    assert.match(statusRow(), /recording is active/i);
    assert.equal(deadlines[0].ms, 120_000);

    currentTabs = { ...currentTabs, A: tab("A", "session-A"), B: tab("B", "session-B-focused") };
    await render();
    await flushAct();
    await stopByHost("lease-valid");
    assert.equal(current().state.phase, "stopping");
    assert.equal(streams[0].track.readyState, "ended", "selected stream releases immediately on STOP request");
    assert.equal(streams[0].track.stopCalls, 1, "one active clip releases its microphone exactly once");
    assert.equal(invokeCount("local_recording_capture_stopped"), 0, "indicator waits for MediaRecorder stop event");
    await stopRecorder(0);
    assert.equal(current().state.phase, "stopping", "real browser stop and track release still wait for typed host cleanup acknowledgement");
    assert.equal(invokeCount("local_recording_capture_stopped"), 1);
    assert.equal(invokeCount("local_recording_commit"), 0);
    nativeStopAcks[0].resolve(undefined);
    await flushAct();
    assert.equal(current().state.phase, "transcribing");
    assert.equal(invokeCount("local_recording_commit"), 1);
    const sendsA = readChatSends("A");
    assert.equal(sendsA.length, 1, "one optimistic owner row is created in frozen A");
    assert.match(sendsA[0].content, /voice/i);
    assert.equal(readChatSends("B").length, 0, "focus changes do not retarget the clip");
    const validToken = String(invocations.find((item) => item.command === "local_recording_commit")?.args.clientToken);
    commitCompletions[0].resolve({
      send_id: "send-valid",
      tab_id: "A",
      state: "delivered",
      error: null,
      ledger_id: "ledger-valid",
      sequence: 1,
      retained_from: 1,
      correlationId: validToken,
    });
    await flushAct();
    assert.equal(acceptedStatuses.length, 1, "one returned owner status reconciles the optimistic row");
    assert.equal(current().state.phase, "armed");

    await act(async () => emit("local-recording-status", recordingStatus(13)));
    await startCapture(lease("lease-access-loss", 13));
    await emitRecorderStarted(1);
    await stopByHost("lease-access-loss");
    await stopRecorder(1);
    assert.equal(current().state.phase, "stopping");
    nativeStopAcks[1].resolve(undefined);
    await flushAct();
    assert.equal(current().state.phase, "transcribing");
    assert.equal(invokeCount("local_recording_commit"), 2);
    const staleToken = String(invocations.filter((item) => item.command === "local_recording_commit")[1].args.clientToken);
    await act(async () => emit("local-recording-capture-invalidated", {
      pluginId: "lark", generation: 13, reason: "accessLost",
    }));
    assert.equal(current().state.phase, "error");
    assert.match(alertText(), /access was lost/i);
    assert.equal(readChatSends("A").some((send) => send.correlationId === staleToken), false);
    commitCompletions[1].resolve({
      send_id: "send-stale",
      tab_id: "A",
      state: "delivered",
      error: null,
      ledger_id: "ledger-stale",
      sequence: 1,
      retained_from: 1,
      correlationId: staleToken,
    });
    await flushAct();
    assert.equal(acceptedStatuses.length, 1, "invalidated transcription completion cannot deliver a status");

    await act(async () => emit("local-recording-status", recordingStatus(14)));
    await startCapture(lease("lease-removal", 14));
    await emitRecorderStarted(2);
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
    await startCapture(lease("lease-deadline", 17));
    await emitRecorderStarted(4);
    await act(async () => deadlines[deadlines.length - 1].callback());
    assert.equal(current().state.phase, "stopping");
    assert.equal(streams[4].track.readyState, "ended", "deadline immediately releases the selected stream");
    assert.equal(streams[4].track.stopCalls, 1, "deadline releases its microphone exactly once");
    await stopRecorder(4);
    assert.equal(current().state.phase, "error");
    assert.match(alertText(), /two minute/i);
    assert.equal(invokeCount("local_recording_commit"), 2, "deadline discards and does not retry");

    await act(async () => emit("local-recording-status", recordingStatus(18)));
    await startCapture(lease("lease-typed-failure", 18));
    await emitRecorderStarted(5);
    await stopByHost("lease-typed-failure");
    await stopRecorder(5);
    assert.equal(current().state.phase, "stopping");
    nativeStopAcks[2].resolve(undefined);
    await flushAct();
    assert.equal(invokeCount("local_recording_commit"), 3);
    const failedToken = String(invocations.filter((item) => item.command === "local_recording_commit")[2].args.clientToken);
    commitCompletions[2].reject(new Error("recording_transcription_failed: transcriber unavailable"));
    await flushAct();
    assert.equal(current().state.phase, "error");
    assert.equal(readChatSends("A").some((send) => send.correlationId === failedToken), false);
    assert.equal(acceptedStatuses.length, 1, "typed transcription error removes its optimistic row without delivery");

    await act(async () => emit("local-recording-status", recordingStatus(19)));
    await startCapture(lease("lease-ui-cancel", 19));
    await emitRecorderStarted(6);
    const cancel = host.querySelector<HTMLButtonElement>('button[aria-label="Cancel hardware voice recording"]');
    assert(cancel, "active hardware capture has an accessible cancel control");
    await act(async () => cancel.click());
    assert.equal(current().state.phase, "stopping");
    await stopRecorder(6);
    assert.equal(current().state.phase, "error");
    assert.match(alertText(), /cancelled/i);
    assert.equal(uploads.length, 3, "only clips that reach transcription are uploaded");
    console.log("useLarkCapture: 9 wiring fixtures passed");
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
