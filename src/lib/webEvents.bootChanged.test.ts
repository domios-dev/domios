import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { configureDaemonTransport } from "./daemonBootstrap";
import { clearRelayTarget, setRelayTarget } from "./webApi";
import { bindWebEventsTransport, resetWebEvents, webListen, __resetWebEventsForTest } from "./webEvents";
import { WEB_BOOT_CHANGED_EVENT, WEB_EVENTS_READY_EVENT, type BootChangedDetail } from "./buildDriftReload";
import type { TransportClient } from "./transportClient";
import { verifyStreamMuxes } from "./streamMux";
import { TERMINAL_VIEWER_DISCONNECTED_EVENT, TERMINAL_VIEWER_RECONNECTED_EVENT } from "./terminalViewerEvents";

vi.mock("./streamMux", async importOriginal => ({
  ...await importOriginal<typeof import("./streamMux")>(),
  verifyStreamMuxes: vi.fn(() => Promise.resolve(true)),
  wakeStreamMuxes: vi.fn(),
  streamMuxDiagnostics: () => [],
}));

interface Subscription {
  path: string;
  emit(frame: unknown): void;
  ready(): void;
  activity(): void;
  signal?: AbortSignal;
}

function fakeClient(subscriptions: Subscription[]): TransportClient {
  return {
    origin: () => window.location.origin,
    subscribe(path: string, onEvent: (name: string, data: unknown) => void, opts?: { signal?: AbortSignal; onReady?: () => void; onActivity?: () => void; onEnd?: (end: {reason: "abort"}) => void }) {
      subscriptions.push({ path, emit: frame => onEvent("message", frame), ready: () => opts?.onReady?.(), activity: () => opts?.onActivity?.(), signal: opts?.signal });
      opts?.signal?.addEventListener("abort", () => opts.onEnd?.({ reason: "abort" }), { once: true });
      return () => {};
    },
  } as unknown as TransportClient;
}

const bootChanged = (build?: string) => ({
  event_type: "overflow",
  data: { reason: "boot-changed", boot: "boot-2", next_seq: 1, ...(build === undefined ? {} : { build }) },
});

describe("webEvents boot-changed detail", () => {
  let subscriptions: Subscription[];
  let details: BootChangedDetail[];
  let readies: number;
  const latest = () => subscriptions[subscriptions.length - 1]!;
  const onBoot = (event: Event) => details.push((event as CustomEvent<BootChangedDetail>).detail);
  const onReady = () => { readies += 1; };

  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(verifyStreamMuxes).mockReset().mockResolvedValue(true);
    configureDaemonTransport({ origin: window.location.origin, token: "fixture-token" });
    __resetWebEventsForTest();
    clearRelayTarget();
    subscriptions = [];
    details = [];
    readies = 0;
    window.addEventListener(WEB_BOOT_CHANGED_EVENT, onBoot);
    window.addEventListener(WEB_EVENTS_READY_EVENT, onReady);
    bindWebEventsTransport(fakeClient(subscriptions));
    webListen("tab-focused", () => {});
  });
  afterEach(() => {
    window.removeEventListener(WEB_BOOT_CHANGED_EVENT, onBoot);
    window.removeEventListener(WEB_EVENTS_READY_EVENT, onReady);
    clearRelayTarget();
    __resetWebEventsForTest();
    vi.useRealTimers();
  });

  it("forwards the daemon build from a local overflow frame", () => {
    latest().emit(bootChanged("daemon-build"));
    expect(details).toEqual([{ boot: "boot-2", build: "daemon-build", relayed: false }]);
  });

  it("checkpoints the latest applied cursor on pagehide without per-event storage", () => {
    sessionStorage.clear();
    const write = vi.spyOn(sessionStorage, "setItem");
    for (let seq = 1; seq <= 100; seq++) latest().emit({ boot: "checkpoint", seq, event_type: "tab-focused", data: {} });
    expect(write).not.toHaveBeenCalled();
    window.dispatchEvent(new Event("pagehide"));
    expect(write).toHaveBeenCalledExactlyOnceWith(expect.stringContaining("codeterm-web-events-cursor:"), "checkpoint:100");
    write.mockRestore();
  });
  it("omits the build when the frame carries none", () => {
    latest().emit(bootChanged());
    expect(details).toEqual([{ boot: "boot-2", relayed: false }]);
    expect("build" in details[0]!).toBe(false);
  });
  it("marks a boot-changed frame from a relayed peer stream", () => {
    setRelayTarget("peer-1", "Peer");
    resetWebEvents();
    const relayed = latest();
    expect(relayed.path).toContain("/api/mesh/relay-stream");
    relayed.emit(bootChanged("peer-build"));
    expect(details).toEqual([{ boot: "boot-2", build: "peer-build", relayed: true }]);
  });
  it("signals readiness only for a reconnect, not the first connection", () => {
    latest().ready();
    expect(readies).toBe(0);
    resetWebEvents();
    latest().ready();
    expect(readies).toBe(1);
  });
  it("restores a quiet stream on authenticated readiness without waiting for application traffic", () => {
    const restored = vi.fn();
    window.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, restored);
    try {
      resetWebEvents();
      latest().ready();
      expect(restored).toHaveBeenCalledTimes(1);
      latest().activity();
      latest().emit({ event_type: "tab-focused", data: {} });
      expect(restored).toHaveBeenCalledTimes(1);
    } finally { window.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, restored); }
  });
  it("accepts keepalive activity as recovery without inventing an application event", () => {
    const restored = vi.fn();
    window.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, restored);
    try {
      resetWebEvents();
      latest().activity();
      expect(restored).toHaveBeenCalledTimes(1);
    } finally { window.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, restored); }
  });
  it("replaces only a silent event channel after carrier proof, retaining its cursor without a lost notice", async () => {
    let settle!: (alive: boolean) => void;
    vi.mocked(verifyStreamMuxes).mockReturnValue(new Promise(resolve => { settle = resolve; }));
    const stream = latest();
    stream.emit({ boot: "boot", seq: 42, event_type: "tab-focused", data: {} });
    stream.ready();
    const lost = vi.fn();
    window.addEventListener(TERMINAL_VIEWER_DISCONNECTED_EVENT, lost);
    try {
    await vi.advanceTimersByTimeAsync(30_000);
    expect(verifyStreamMuxes).toHaveBeenCalledWith(window.location.origin);
    expect(stream.signal?.aborted).toBe(false);
    settle(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(stream.signal?.aborted).toBe(true);
    expect(subscriptions).toHaveLength(2);
    expect(new URL(latest().path, window.location.origin).searchParams.get("since")).toBe("boot:42");
    expect(lost).not.toHaveBeenCalled();
    } finally { window.removeEventListener(TERMINAL_VIEWER_DISCONNECTED_EVENT, lost); }
  });
  it("aborts only after a failed carrier proof", async () => {
    vi.mocked(verifyStreamMuxes).mockResolvedValue(false);
    const stream = latest();
    stream.ready();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(stream.signal?.aborted).toBe(true);
    expect(stream.signal?.reason.name).toBe("TimeoutError");
  });
  it("ignores an old proof after a target reset", async () => {
    let settle!: (alive: boolean) => void;
    vi.mocked(verifyStreamMuxes).mockReturnValue(new Promise(resolve => { settle = resolve; }));
    latest().ready();
    await vi.advanceTimersByTimeAsync(30_000);
    resetWebEvents();
    const replacement = latest();
    replacement.ready();
    settle(false);
    await vi.advanceTimersByTimeAsync(0);
    expect(replacement.signal?.aborted).toBe(false);
  });
  it("new delivered bytes defeat a stale failed proof", async () => {
    let settle!: (alive: boolean) => void;
    vi.mocked(verifyStreamMuxes).mockReturnValue(new Promise(resolve => { settle = resolve; }));
    const stream = latest();
    stream.ready();
    await vi.advanceTimersByTimeAsync(30_000);
    stream.activity();
    settle(false);
    await vi.advanceTimersByTimeAsync(0);
    expect(stream.signal?.aborted).toBe(false);
  });
});
