import { configureDaemonTransport } from "./daemonBootstrap";
import { configureRemoteTabTransport } from "./remoteTabTransport";
import { afterEach, describe, expect, finish, it, vi } from "../test/standaloneTest";
import {
  onRemoteTabEventSignal, remoteTabEventSubscriptionSnapshot, resumeRemoteTabEvents, subscribeRemoteTabEvent,
  waitForRemoteTabEventGenerationSettlement,
} from "./remoteTabEvents";
import { onRemoteTransportState } from "./remoteTabTransportState";

configureDaemonTransport({ origin: "http://127.0.0.1:7685", token: "fixture" });
configureRemoteTabTransport({mesh_tailnet_host: null, cors_allow_origins: [], preview_secret_paths: [],  enabled: true, host: "127.0.0.1", port: 7685, token: "fixture", cors: false });

describe("remote chat event subscription ownership", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("forwards runtime faults only from the owning remote tab and remaps its identity", async () => {
    const state = { state: "degraded", epoch: 2, reason: "fixture pipe failure" };
    const frames = [
      { boot: "runtime-boot", seq: 1, event_type: "agent-runtime-state", data: { tab_id: "other", session_binding_nonce: "foreign", state } },
      { boot: "runtime-boot", seq: 2, event_type: "agent-runtime-state", data: { tab_id: "tab", session_binding_nonce: "bound", state } },
    ];
    vi.stubGlobal("fetch", async () => new Response(new ReadableStream({
      start(controller) { controller.enqueue(new TextEncoder().encode(frames.map((frame) => `data: ${JSON.stringify(frame)}\n\n`).join(""))); },
    })));
    const received: unknown[] = [];
    let resolve!: () => void;
    const delivered = new Promise<void>((done) => { resolve = done; });
    const stop = subscribeRemoteTabEvent("agent-runtime-state:mesh:peer/tab", ({ payload }) => { received.push(payload); resolve(); });
    try {
      await delivered;
      expect(received).toEqual([{ tab_id: "mesh:peer/tab", session_binding_nonce: "bound", state }]);
    } finally { stop!(); }
  });

  it("forwards provider switch phases only from the owning remote tab", async () => {
    const status = { operation_id: "operation-token", provider: "target", phase: "running", outcome: null, error: null };
    const frames = [
      { boot: "switch-boot", seq: 1, event_type: "provider-switch-status", data: { ...status, tab_id: "other" } },
      { boot: "switch-boot", seq: 2, event_type: "provider-switch-status", data: { ...status, tab_id: "tab" } },
    ];
    vi.stubGlobal("fetch", async () => new Response(new ReadableStream({
      start(controller) { controller.enqueue(new TextEncoder().encode(frames.map((frame) => `data: ${JSON.stringify(frame)}\n\n`).join(""))); },
    })));
    const received: unknown[] = [];
    let resolve!: () => void;
    const delivered = new Promise<void>((done) => { resolve = done; });
    const stop = subscribeRemoteTabEvent("provider-switch-status:mesh:peer/tab", ({ payload }) => { received.push(payload); resolve(); });
    try {
      await delivered;
      expect(received).toEqual([{ ...status, tab_id: "mesh:peer/tab" }]);
    } finally { stop!(); }
  });

  it("signals provider switch stream readiness, overflow and loss", async () => {
    const event = "provider-switch-status:mesh:peer/tab";
    let stream!: ReadableStreamDefaultController<Uint8Array>;
    vi.stubGlobal("fetch", async () => new Response(new ReadableStream({ start(controller) { stream = controller; } })));
    const received: string[] = [];
    const waits = new Map<string, () => void>();
    const when = (signal: string) => new Promise<void>((resolve) => waits.set(signal, resolve));
    const ready = when("ready");
    const stopSignals = onRemoteTabEventSignal(event, (signal) => {
      received.push(signal);
      waits.get(signal)?.();
    });
    const stop = subscribeRemoteTabEvent(event, () => undefined)!;
    try {
      await ready;
      const resynced = when("resync");
      stream.enqueue(new TextEncoder().encode('data: {"boot":"b","seq":1,"event_type":"overflow","data":{}}\n\n'));
      await resynced;
      const lost = when("lost");
      stream.error(new Error("peer stream lost"));
      await lost;
      expect(received).toEqual(["ready", "resync", "lost"]);
    } finally {
      stop();
      stopSignals();
    }
  });

  it("resume does not supersede a first-frame-awaiting connection; unsubscribe owns it", async () => {
    // In-flight fetch, no frame delivered yet: resume must leave it alone rather
    // than restart it mid initial replay (the churn race that blanked panes).
    vi.stubGlobal("fetch", vi.fn().mockReturnValue(new Promise(() => {})));
    const event = "tab-chat-msg-mesh:peer/tab";
    const unsubscribe = subscribeRemoteTabEvent(event, () => undefined)!;
    expect(remoteTabEventSubscriptionSnapshot(event)?.generation).toBe(1);
    resumeRemoteTabEvents();
    expect(remoteTabEventSubscriptionSnapshot(event)?.generation).toBe(1);
    expect(remoteTabEventSubscriptionSnapshot(event)).toMatchObject({ generation: 1, hasController: true });
    unsubscribe();
    expect(remoteTabEventSubscriptionSnapshot(event)).toBeNull();
    vi.unstubAllGlobals();
  });

  it("keeps live streams open on resume and restores parked streams from their cursor", async () => {
    vi.stubGlobal("window", new EventTarget());
    const bytes = new TextEncoder().encode(
      'data: {"boot":"boot-a","seq":9,"event_type":"tab-chat-msg-tab","data":{"delta":"x"}}\n\n',
    );
    const requests: string[] = [];
    let stream!: ReadableStreamDefaultController<Uint8Array>;
    const fetchMock: typeof fetch = async (input) => {
      requests.push(String(input));
      return new Response(new ReadableStream({
        start(controller) { stream = controller; controller.enqueue(bytes); },
      }));
    };
    vi.stubGlobal("fetch", fetchMock);
    const available: string[] = [];
    const stopState = onRemoteTransportState(({ availability }) => available.push(availability));
    let delivered!: () => void;
    const delivery = new Promise<void>((resolve) => { delivered = resolve; });
    const event = "tab-chat-msg-mesh:peer/tab";
    const first = subscribeRemoteTabEvent(event, () => delivered())!;
    await delivery;
    expect(remoteTabEventSubscriptionSnapshot(event)?.cursor).toBe("boot-a:9");
    expect(available).toContain("available");
    resumeRemoteTabEvents();
    expect(requests.length).toBe(1);
    const ended = waitForRemoteTabEventGenerationSettlement(event, 1);
    stream.error(new Error("peer stream lost"));
    await ended;
    resumeRemoteTabEvents();
    expect(requests[1]).toContain("since=boot-a%3A9");
    const second = subscribeRemoteTabEvent(event, () => undefined)!;
    first();
    expect(remoteTabEventSubscriptionSnapshot(event)).not.toBeNull();
    second();
    stopState();
    vi.unstubAllGlobals();
  });

  it("signals Chat recovery after a generation change and adopts B's overflow boundary", async () => {
    vi.stubGlobal("window", new EventTarget());
    const event = "tab-chat-msg-mesh:peer/tab";
    const streams: ReadableStreamDefaultController<Uint8Array>[] = [];
    vi.stubGlobal("fetch", async () => new Response(new ReadableStream<Uint8Array>({
      start(controller) { streams.push(controller); },
    })));
    let resolveFrame!: () => void;
    const frame = new Promise<void>(resolve => { resolveFrame = resolve; });
    const signals: string[] = [];
    let resolveResync!: () => void;
    const nextResync = () => new Promise<void>(resolve => { resolveResync = resolve; });
    const stopSignals = onRemoteTabEventSignal(event, signal => {
      signals.push(signal);
      resolveResync();
    });
    const stop = subscribeRemoteTabEvent(event, () => resolveFrame())!;
    const encode = (value: object) => new TextEncoder().encode(`data: ${JSON.stringify(value)}\n\n`);
    try {
      streams[0].enqueue(encode({ event_type: "tab-chat-msg-tab", data: { tab_id: "tab" }, boot: "old", seq: 1 }));
      await frame;
      const reconnect = nextResync();
      streams[0].close();
      await reconnect;
      const reset = nextResync();
      streams[1].enqueue(encode({ event_type: "overflow", data: { boot: "new", next_seq: 7 } }));
      await reset;
      expect(signals).toEqual(["resync", "resync"]);
      expect(remoteTabEventSubscriptionSnapshot(event)).toMatchObject({ generation: 2, cursor: "new:6" });
    } finally {
      stop();
      stopSignals();
    }
  });

  it("authoritative snapshot then live events delivered", async () => {
    vi.stubGlobal("window", new EventTarget());
    // Frame 1 = authoritative snapshot; 2/4 = live deltas; replayed seq-2 dedups (G3).
    const frames = [
      '{"boot":"b","seq":1,"event_type":"tab-chat-msg-tab","data":{"delta":"snapshot"}}',
      '{"boot":"b","seq":2,"event_type":"tab-chat-msg-tab","data":{"delta":"live-1"}}',
      '{"boot":"b","seq":2,"event_type":"tab-chat-msg-tab","data":{"delta":"dup"}}',
      '{"boot":"b","seq":3,"event_type":"tab-chat-msg-tab","data":{"delta":"live-2"}}',
    ];
    const bytes = new TextEncoder().encode(frames.map((f) => `data: ${f}\n\n`).join(""));
    const fetchMock: typeof fetch = async () => new Response(new ReadableStream({
      start(controller) { controller.enqueue(bytes); controller.close(); },
    }));
    vi.stubGlobal("fetch", fetchMock);
    const available: string[] = [];
    const stopState = onRemoteTransportState(({ channel, availability }) => {
      if (channel === "chat") available.push(availability);
    });
    const deltas: string[] = [];
    let resolveThird!: () => void;
    const gotThird = new Promise<void>((resolve) => { resolveThird = resolve; });
    const event = "tab-chat-msg-mesh:peer/tab";
    const unsubscribe = subscribeRemoteTabEvent<{ delta: string }>(event, ({ payload }) => {
      deltas.push(payload.delta);
      if (deltas.length === 3) resolveThird();
    })!;
    await gotThird;
    expect(deltas).toEqual(["snapshot", "live-1", "live-2"]);
    expect(available[0]).toBe("available");
    expect(remoteTabEventSubscriptionSnapshot(event)?.cursor).toBe("b:3");
    unsubscribe();
    stopState();
    vi.unstubAllGlobals();
  });

  it("delivers peer resize-owner changes with the composite tab and PTY identity", async () => {
    vi.stubGlobal("window", new EventTarget());
    const frame = JSON.stringify({
      boot: "boot-r",
      seq: 1,
      event_type: "pty-resize-owner-changed",
      data: { tab_id: "tab", pty_id: "pty-1", client_id: "web:other", cols: 80, rows: 24 },
    });
    vi.stubGlobal("fetch", async () => new Response(new ReadableStream({
      start(controller) { controller.enqueue(new TextEncoder().encode(`data: ${frame}\n\n`)); },
    })));
    const event = "pty-resize-owner-changed:mesh:peer/tab";
    let payload: Record<string, unknown> | null = null;
    let resolveDelivered!: () => void;
    const delivered = new Promise<void>((resolve) => { resolveDelivered = resolve; });
    const unsubscribe = subscribeRemoteTabEvent<Record<string, unknown>>(event, (value) => {
      payload = value.payload;
      resolveDelivered();
    })!;
    await delivered;
    expect(payload).toMatchObject({
      tab_id: "mesh:peer/tab",
      pty_id: "mesh:peer/tab",
      client_id: "web:other",
    });
    expect(remoteTabEventSubscriptionSnapshot(event)?.cursor).toBe("boot-r:1");
    unsubscribe();
    vi.unstubAllGlobals();
  });
});

let completed = false;
it("completes all event barriers", () => { completed = true; });
process.once("beforeExit", () => { if (!completed) process.exitCode = 1; });
finish();
