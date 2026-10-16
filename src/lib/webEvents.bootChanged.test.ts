import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { configureDaemonTransport } from "./daemonBootstrap";
import { clearRelayTarget, setRelayTarget } from "./webApi";
import { bindWebEventsTransport, resetWebEvents, webListen, __resetWebEventsForTest } from "./webEvents";
import { WEB_BOOT_CHANGED_EVENT, WEB_EVENTS_READY_EVENT, type BootChangedDetail } from "./buildDriftReload";
import type { TransportClient } from "./transportClient";

interface Subscription {
  path: string;
  emit(frame: unknown): void;
  ready(): void;
}

function fakeClient(subscriptions: Subscription[]): TransportClient {
  return {
    origin: () => window.location.origin,
    subscribe(path: string, onEvent: (name: string, data: unknown) => void, opts?: { onReady?: () => void }) {
      subscriptions.push({ path, emit: frame => onEvent("message", frame), ready: () => opts?.onReady?.() });
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
  });

  it("forwards the daemon build from a local overflow frame", () => {
    latest().emit(bootChanged("daemon-build"));
    expect(details).toEqual([{ boot: "boot-2", build: "daemon-build", relayed: false }]);
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
});
