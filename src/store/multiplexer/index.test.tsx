// Run: npx tsx src/store/multiplexer/index.test.tsx
// codeterm#336 part B: mux_get_state call counts against an injected fake, gated on observed signals only.

import type { RemotePeerTabs } from "../../lib/remoteTabPortal";
import { RELAY_PROTOCOL_VERSION, type MeshPeersResponse, type MultiplexerState, type Tab } from "../../lib/types";

process.env.VITE_CODETERM_TEST = "1";

class FakeStorage {
  private map = new Map<string, string>();
  getItem(key: string): string | null {
    return this.map.has(key) ? this.map.get(key)! : null;
  }
  setItem(key: string, value: string): void {
    this.map.set(key, value);
  }
  removeItem(key: string): void {
    this.map.delete(key);
  }
  clear(): void {
    this.map.clear();
  }
}

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { __BUILD_HASH__?: string }).__BUILD_HASH__ = "mux-renewal-test";
(globalThis as unknown as { localStorage: FakeStorage }).localStorage = new FakeStorage();
(globalThis as unknown as { sessionStorage: FakeStorage }).sessionStorage = new FakeStorage();

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function setupHappyDom() {
  const React = await import("react");
  const { act } = React;
  const { createRoot } = await import("react-dom/client");
  const { Window } = await import("happy-dom");

  const window = new Window({ url: "http://localhost:7686/" });
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "HTMLButtonElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
    "CustomEvent",
  ] as const) {
    Object.defineProperty(globalThis, key, {
      value: (window as unknown as Record<string, unknown>)[key],
      configurable: true,
    });
  }

  Object.defineProperty(globalThis, "requestAnimationFrame", {
    configurable: true,
    value: (callback: FrameRequestCallback) => { callback(0); return 0; },
  });

  return { act, createRoot, React, window };
}

type ActFn = Awaited<ReturnType<typeof setupHappyDom>>["act"];

/** Repeatedly flushes the microtask queue until `condition()` holds, failing
 * deterministically past a bounded safety deadline instead of hanging. */
async function waitUntil(condition: () => boolean, act: ActFn, maxTurns = 500): Promise<void> {
  for (let i = 0; i < maxTurns; i++) {
    if (condition()) return;
    await act(async () => {
      await Promise.resolve();
    });
  }
  throw new Error(`waitUntil: condition not met within ${maxTurns} microtask turns`);
}

function sampleState(wsId: string) {
  return {
    workspaces: [{ id: wsId, name: "Renewal", tabs: [], active_tab_id: null, cwd: null }],
    tabs: {},
    teams: [],
    active_workspace_id: wsId,
    focused_tab_id: null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 1,
    pendingCloses: [],
  };
}

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
}

const onlinePeer: MeshPeersResponse["paired"][number] = {
  instance_id: "peer",
  instance_name: "Peer",
  addresses: [],
  api_port: 7685,
  public_key: "",
  paired_at: "2026-01-01T00:00:00Z",
  last_seen: null,
  online: true,
  is_primary: false,
  access_level: "full",
  web_ui: null,
  relay_protocol_version: 1,
  last_active_ms: null,
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((r, fail) => {
    resolve = r;
    reject = fail;
  });
  return { promise, resolve, reject };
}

/** `mux_get_state` responses stay pending until explicitly resolved; calls
 * are counted synchronously at invocation, before any settling. */
async function mountProvider(options: {
  refuseInitialMux?: boolean;
  refuseInitialOrdinary?: boolean;
  storageUnavailableInitialMux?: boolean;
  pairedPeerCount?: number;
  roster?: () => MeshPeersResponse["paired"];
  peerMux?: (peerId: string) => Promise<MultiplexerState>;
  selectedLeaf?: boolean;
  ordinaryTabs?: () => Tab[];
} = {}) {
  (globalThis as unknown as { localStorage: FakeStorage }).localStorage.clear();
  (globalThis as unknown as { sessionStorage: FakeStorage }).sessionStorage.clear();

  const { act, createRoot, React, window } = await setupHappyDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { MultiplexerProvider, STORAGE_UNAVAILABLE_MESSAGE, useMuxState, useMultiplexer, usePairedWorkspaceInventory } = await import("./index");

  let actions!: ReturnType<typeof useMultiplexer>;
  const pairCalls: Record<string, unknown>[] = [];
  let latestState!: ReturnType<typeof useMuxState>;
  let inventory!: ReturnType<typeof usePairedWorkspaceInventory>;
  const focusCalls: string[] = [];
  function StorageStatusProbe() {
    actions = useMultiplexer();
    const state = useMuxState();
    latestState = state;
    inventory = usePairedWorkspaceInventory();
    return React.createElement(
      "div",
      { "data-testid": "storage-unavailable-probe" },
      state.storageUnavailable ? STORAGE_UNAVAILABLE_MESSAGE : "",
    );
  }

  let muxGetStateCalls = 0;
  let ordinaryTabsCalls = 0;
  let refusedInitialMux = false;
  let refusedInitialOrdinary = false;
  const pendingMuxGetState: Array<Deferred<unknown>> = [];
  const eventListeners = new Map<string, Set<(event: { payload: unknown }) => void>>();
  __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    if (cmd === "mux_create_orchestration_pair") {
      pairCalls.push(args ?? {});
      if (args?.callerTabId) throw new Error("leaf caller refused");
      return { orchestrator_tab_id: "new-lead", planner_tab_id: "new-planner" } as T;
    }
    if (cmd === "mesh_list_peers" && (options.roster || options.pairedPeerCount !== undefined)) {
      const response: MeshPeersResponse = {
        paired: options.roster?.() ?? Array.from({ length: options.pairedPeerCount ?? 0 }, () => onlinePeer),
        local_instance_name: "Local",
      };
      return response as T;
    }
    if (cmd === "mesh_peer_mux_state" && options.peerMux) return await options.peerMux(String(args?.peerId)) as T;
    if (cmd === "mux_set_focused_tab") focusCalls.push(String(args?.tabId));
    if (cmd === "mesh_peer_tabs_list") {
      const peerId = args?.peerId;
      if (typeof peerId !== "string" || (!options.roster && peerId !== onlinePeer.instance_id)) throw new Error(`Unexpected peer tab read: ${peerId}`);
      const response: RemotePeerTabs = {
        peer_id: peerId, peer_name: onlinePeer.instance_name,
        tabs: options.roster ? [peerMain(peerId)] : [],
      };
      return response as T;
    }
    if (cmd === "mux_get_state") {
      muxGetStateCalls++;
      if (options.refuseInitialMux && !refusedInitialMux) {
        refusedInitialMux = true;
        return Promise.reject(new Error("state snapshot unavailable while tab state is initializing or changing"));
      }
      if (options.storageUnavailableInitialMux && !refusedInitialMux) {
        refusedInitialMux = true;
        return Promise.reject(new Error(STORAGE_UNAVAILABLE_MESSAGE));
      }
      const d = deferred<unknown>();
      pendingMuxGetState.push(d);
      return d.promise as Promise<T>;
    }
    if (cmd === "list_notifications") return [] as unknown as T;
    if (cmd === "ordinary_tabs_list") {
      ordinaryTabsCalls++;
      if (options.refuseInitialOrdinary && !refusedInitialOrdinary) {
        refusedInitialOrdinary = true;
        return Promise.reject(new Error("tab summary unavailable while tab state is initializing or changing"));
      }
      return (options.ordinaryTabs?.() ?? []) as unknown as T;
    }
    return undefined as unknown as T;
  });
  __setListenForTest(async <T,>(event: string, handler: (event: { payload: T }) => void) => {
    const listeners = eventListeners.get(event) ?? new Set();
    listeners.add(handler as (event: { payload: unknown }) => void);
    eventListeners.set(event, listeners);
    return () => listeners.delete(handler as (event: { payload: unknown }) => void);
  });

  // Structural spy over the owned scheduler seam: counts real
  // window.setInterval registrations without faking their behavior.
  let intervalCalls = 0;
  const realSetInterval = window.setInterval.bind(window);
  Object.defineProperty(window, "setInterval", {
    configurable: true,
    value: (...args: Parameters<typeof realSetInterval>) => {
      intervalCalls++;
      return realSetInterval(...args);
    },
  });

  const host = window.document.createElement("div");
  window.document.body.appendChild(host);
  const root = createRoot(host as unknown as Element);
  await act(async () => {
    root.render(React.createElement(MultiplexerProvider, null, React.createElement(StorageStatusProbe)));
  });

  const settleMuxGetState = async () => {
    const pending = pendingMuxGetState.splice(0, pendingMuxGetState.length);
    if (pending.length === 0) return;
    await act(async () => {
      for (const d of pending) {
        const snapshot = sampleState("ws-renewal");
        if (options.roster) snapshot.workspaces[0] = Object.assign(snapshot.workspaces[0], { protected: true });
        d.resolve(options.selectedLeaf ? { ...snapshot, focused_tab_id: "manager",
          tabs: { manager: { id: "manager", workspace_id: "ws-renewal", role: "Manager", agent_type: "provider", session_binding_nonce: "manager-nonce" } },
          workspaces: [{ ...snapshot.workspaces[0], tabs: ["manager"], active_tab_id: "manager" }],
        } : snapshot);
      }
      await Promise.all(pending.map((d) => d.promise));
    });
  };
  // The listener-bind promise resolving is a real, observable settle point
  // for the mount-time effect chain; the first authoritative read follows it.
  await waitUntil(
    () => Boolean((window as unknown as { __ctEvents?: { reconnects: number } }).__ctEvents?.reconnects),
    act,
  );
  await settleMuxGetState();

  return {
    act,
    window,
    host,
    pairCalls,
    focusCalls,
    get inventory() { return inventory; },
    get focusedTabId() { return latestState.focused_tab_id; },
    get actions() { return actions; },
    get muxGetStateCalls() {
      return muxGetStateCalls;
    },
    get ordinaryTabsCalls() {
      return ordinaryTabsCalls;
    },
    get intervalCalls() {
      return intervalCalls;
    },
    get tabIds() {
      return Object.keys(latestState.tabs);
    },
    settleMuxGetState,
    rejectMuxGetState(error: unknown) {
      for (const pending of pendingMuxGetState.splice(0)) pending.reject(error);
    },
    emit(event: string, payload: unknown = null) {
      for (const handler of eventListeners.get(event) ?? []) handler({ payload });
    },
    unmount: async () => {
      await act(async () => root.unmount());
      __setInvokeForTest(null);
      __setListenForTest(null);
    },
  };
}

test("owner pair creation never inherits a selected Manager or a claimed picker tab", async () => {
  const harness = await mountProvider({ selectedLeaf: true });
  try {
    for (const existingTabId of [undefined, "picker"]) {
      await harness.act(async () => {
        const result = await harness.actions.createOrchestrationPair({
          workspaceId: "ws-renewal", orchestratorAgent: "provider", plannerAgent: "provider", existingTabId,
        });
        assert(result.orchestrator_tab_id === "new-lead", "owner action must receive the created pair");
      });
      const request = harness.pairCalls[harness.pairCalls.length - 1];
      assert(!("callerTabId" in request), "owner action must carry no agent caller");
      assert(!("sessionBindingNonce" in request), "owner action must carry no agent session claim");
      assert((request.params as Record<string, unknown>).existing_tab_id === existingTabId, "picker reuse is preserved");
    }
    assert(harness.pairCalls.length === 2, "each owner action creates exactly one pair");
  } finally {
    await harness.unmount();
  }
});

test("generation-only mux events reuse cached snapshots and fetch changed generations", async () => {
  const harness = await mountProvider({ pairedPeerCount: 0 });
  try {
    const before = harness.muxGetStateCalls;
    await harness.act(async () => harness.emit("mux-state-changed", { generation: 1 }));
    assert(harness.muxGetStateCalls === before, "cached generation must skip mux_get_state");
    await harness.act(async () => harness.emit("mux-state-changed", { generation: 2 }));
    assert(harness.muxGetStateCalls === before + 1, "new generation must fetch mux_get_state");
    await harness.settleMuxGetState();
  } finally {
    await harness.unmount();
  }
});

test("a cancelled rebind waits for the next accepted peer snapshot", async () => {
  const harness = await mountProvider();
  const receipts: Array<{ success: boolean }> = [];
  const onReady = (event: Event) => receipts.push((event as CustomEvent).detail);
  window.addEventListener("codeterm-peer-switch-ready", onReady);
  try {
    await harness.act(async () => {
      window.dispatchEvent(new CustomEvent("codeterm-peer-switch-reset", { detail: { switchId: 18 } }));
      window.dispatchEvent(new CustomEvent("codeterm-peer-switch-rebind", { detail: { switchId: 18 } }));
      harness.rejectMuxGetState(new DOMException("Peer context changed", "AbortError"));
    });
    assert(receipts.length === 0, "cancellation must not announce a failed peer");
    await harness.act(async () => harness.emit("mux-state-changed", sampleState("ready-shell")));
    assert(Number(receipts.length) === 1 && receipts[0].success, "accepted snapshot must settle the rebind");
  } finally {
    window.removeEventListener("codeterm-peer-switch-ready", onReady);
    await harness.unmount();
  }
});

test("a peer snapshot pushed during rebind announces success before the refresh settles", async () => {
  const harness = await mountProvider();
  const receipts: Array<{ switchId: number; success: boolean }> = [];
  const onReady = (event: Event) => receipts.push((event as CustomEvent).detail);
  window.addEventListener("codeterm-peer-switch-ready", onReady);
  try {
    await harness.act(async () => {
      window.dispatchEvent(new CustomEvent("codeterm-peer-switch-reset", { detail: { switchId: 19 } }));
      window.dispatchEvent(new CustomEvent("codeterm-peer-switch-rebind", { detail: { switchId: 19 } }));
      harness.emit("mux-state-changed", sampleState("ready-shell"));
    });
    assert(receipts.length === 1 && receipts[0].switchId === 19 && receipts[0].success,
      "an accepted current-peer snapshot must announce successful readiness");
    await harness.settleMuxGetState();
    assert(receipts.length === 1, "refresh completion must not announce readiness twice");
  } finally {
    window.removeEventListener("codeterm-peer-switch-ready", onReady);
    await harness.unmount();
  }
});

test("Storage unavailable snapshot exposes its preservation status to the UI", async () => {
  const harness = await mountProvider({ storageUnavailableInitialMux: true });
  const { STORAGE_UNAVAILABLE_MESSAGE } = await import("./index");
  await waitUntil(
    () => harness.host.querySelector("[data-testid='storage-unavailable-probe']")?.textContent === STORAGE_UNAVAILABLE_MESSAGE,
    harness.act,
  );
  assert(
    harness.host.querySelector("[data-testid='storage-unavailable-probe']")?.textContent === STORAGE_UNAVAILABLE_MESSAGE,
    "the unavailable read must produce explicit preserved-data status",
  );
  await harness.unmount();
});

test("healthy stream: no renewal interval, no growth from non-resync events", async () => {
  const harness = await mountProvider();
  const baseline = harness.muxGetStateCalls;
  assert(baseline > 0, "mount should hydrate at least once");
  assert(harness.intervalCalls === 0, `no renewal interval may ever be registered, got ${harness.intervalCalls}`);

  // Arbitrary non-resync events must not touch mux_get_state.
  await harness.act(async () => {
    window.dispatchEvent(new Event("codeterm-tab-resize"));
    window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "extensions" } }));
    window.dispatchEvent(new Event("some-unrelated-event"));
  });

  assert(
    harness.muxGetStateCalls === baseline,
    `expected no growth in mux_get_state calls, baseline=${baseline} after=${harness.muxGetStateCalls}`,
  );
  assert(harness.intervalCalls === 0, "no renewal interval may be registered afterward either");
  await harness.unmount();
});

test("authoritative snapshot refusal waits for mux-state-changed", async () => {
  const harness = await mountProvider({ refuseInitialMux: true });
  assert(harness.muxGetStateCalls === 1, "the initial authoritative read must be attempted once");

  await harness.act(async () => {
    harness.emit("mux-state-changed");
  });
  await waitUntil(() => harness.muxGetStateCalls === 2, harness.act);
  await harness.settleMuxGetState();
  assert((harness.muxGetStateCalls as number) === 2, "one mux event must drive exactly one recovery read");

  await harness.act(async () => {
    await Promise.resolve();
  });
  assert((harness.muxGetStateCalls as number) === 2, "a refusal must not start a timer or caller-side retry");
  await harness.unmount();
});

test("ordinary tab refusal waits for mesh-state-changed", async () => {
  const harness = await mountProvider({ refuseInitialOrdinary: true });
  assert(harness.ordinaryTabsCalls === 1, "the initial ordinary-tab read must be attempted once");

  await harness.act(async () => {
    harness.emit("mesh-state-changed");
  });
  await waitUntil(() => harness.ordinaryTabsCalls === 2, harness.act);
  assert((harness.ordinaryTabsCalls as number) === 2, "one mesh event must drive exactly one recovery read");

  await harness.act(async () => {
    await Promise.resolve();
  });
  assert((harness.ordinaryTabsCalls as number) === 2, "ordinary tabs must not poll after a refusal");
  await harness.unmount();
});

test("local-only mux pushes skip the ordinary tab list", async () => {
  const harness = await mountProvider({ pairedPeerCount: 0 });
  Object.defineProperty(globalThis, "requestAnimationFrame", { configurable: true, value: () => 1 });
  const baseline = harness.ordinaryTabsCalls;
  for (let generation = 2; generation <= 26; generation++) {
    await harness.act(async () => {
      harness.emit("mux-state-changed", { ...sampleState("ws-renewal"), generation });
    });
  }
  const calls = harness.ordinaryTabsCalls - baseline;
  console.log(`METRIC local-only mux pushes=25 ordinary_tabs_list=${calls}`);
  assert(calls === 0, `local-only pushes must not fetch the ordinary tab list, got ${calls}`);
  await harness.unmount();
});

test("telemetry-only mux pushes skip the ordinary tab list for paired peers", async () => {
  const harness = await mountProvider({ pairedPeerCount: 1 });
  Object.defineProperty(globalThis, "requestAnimationFrame", { configurable: true, value: () => 1 });
  const baseline = harness.ordinaryTabsCalls;
  for (let generation = 2; generation <= 26; generation++) {
    await harness.act(async () => {
      harness.emit("mux-state-changed", { ...sampleState("ws-renewal"), generation });
    });
  }
  const calls = harness.ordinaryTabsCalls - baseline;
  assert(calls === 0, `telemetry-only pushes must not fetch the ordinary tab list, got ${calls}`);
  await harness.unmount();
});

test("a remote peer tab appears on remote-workers-changed and leaves on mesh-state-changed", async () => {
  const remoteId = "mesh:peer/worker";
  let peerTabs: Tab[] = [];
  const harness = await mountProvider({ pairedPeerCount: 1, ordinaryTabs: () => peerTabs });
  try {
    assert(!harness.tabIds.includes(remoteId), "no remote tab before the peer registers one");

    peerTabs = [{ id: remoteId, title: "Worker", pty_id: null, tab_type: "Terminal", cwd: null,
      workspace_id: "ws-renewal", remote_peer_id: onlinePeer.instance_id, remote_tab_id: "worker", parent_tab_id: "lead" }];
    await harness.act(async () => { harness.emit("remote-workers-changed", { orchestrator_tab_id: "lead", workers: {} }); });
    assert(harness.tabIds.includes(remoteId), "the worker event adds the online peer's tab");

    peerTabs = [];
    await harness.act(async () => { harness.emit("mesh-state-changed"); });
    assert(!harness.tabIds.includes(remoteId), "the mesh event removes the vanished tab while the peer is still online");
  } finally {
    await harness.unmount();
  }
});

function peerMain(peerId: string): Tab {
  return {
    id: "main", tab_type: "Terminal", title: `General on ${peerId}`, cwd: null, pty_id: null,
    role: "Manager", role_profile: "general_agent", protected: true, workspace_id: "peer-general",
  };
}

function peerSnapshot(peerId: string, project: string): MultiplexerState {
  return {
    ...sampleState("peer-general"),
    workspaces: [
      { id: "peer-general", name: "General", protected: true, tabs: ["main"], active_tab_id: "main", cwd: null },
      { id: "same-workspace", name: project, tabs: [], active_tab_id: null, cwd: null },
    ],
    tabs: { main: peerMain(peerId) },
  };
}

test("paired General opens the verified host tab and reuses attachments with colliding host tab ids", async () => {
  const a = { ...onlinePeer, instance_id: "a", instance_name: "Desk", relay_protocol_version: RELAY_PROTOCOL_VERSION };
  const b = { ...a, instance_id: "b" };
  const old = { ...a, instance_id: "old", relay_protocol_version: RELAY_PROTOCOL_VERSION - 1 };
  let roster = [a, b, old];
  const harness = await mountProvider({ roster: () => roster, peerMux: async id => peerSnapshot(id, id) });
  try {
    const ids = () => harness.inventory.generals.flatMap(item => item.sessions.map(tab => tab.viewerId));
    assert(ids().join(",") === "mesh:a/main,mesh:b/main", "the same raw Main resolves once per installation");
    assert(harness.inventory.generals[2].availability === "update-required", "an old peer has a card and no session route");
    await harness.act(async () => {
      harness.actions.attachRemoteTab?.("ws-renewal", { peer_id: "a", peer_name: "Desk", tabs: [] }, peerMain("a"));
    });
    await harness.act(async () => harness.actions.setActiveTab("ws-renewal", "mesh:a/main"));
    assert(harness.tabIds.filter(id => id === "mesh:a/main").length === 1, "opening the General reuses the attachment");
    await harness.act(async () => harness.actions.setActiveTab("ws-renewal", "mesh:b/main"));
    assert(harness.focusCalls.join(",") === "mesh:a/main,mesh:b/main", "selection addresses each exact host tab");
    roster = [a, { ...b, online: false }, old];
    await harness.act(async () => harness.emit("mesh-state-changed"));
    assert(ids().includes("mesh:b/main"), "a confirmed offline session stays in place");
    assert(harness.inventory.generals[1].availability === "offline", "the cached card shows offline");
    roster = [a, old];
    await harness.act(async () => harness.emit("mesh-state-changed"));
    assert(!ids().includes("mesh:b/main"), "unpair removes discovery");
    assert(harness.tabIds.includes("mesh:b/main") && harness.focusedTabId === "mesh:b/main", "the open viewer remains selected");
    await harness.act(async () => harness.actions.setActiveTab("ws-renewal", "mesh:b/main"));
    assert(harness.focusCalls.length === 2, "the retained viewer cannot send selection to the unpaired host");
  } finally {
    await harness.unmount();
  }
});

test("late A and B workspace replies cannot change the current A inventory or the selected viewer", async () => {
  const a = { ...onlinePeer, instance_id: "a", relay_protocol_version: RELAY_PROTOCOL_VERSION };
  const b = { ...a, instance_id: "b" };
  const initialA = deferred<MultiplexerState>();
  const initialB = deferred<MultiplexerState>();
  const finalA = deferred<MultiplexerState>();
  const browseReads = [initialA, initialB, finalA];
  let browsing = false;
  const harness = await mountProvider({
    roster: () => [a, b],
    peerMux: id => browsing ? browseReads.shift()!.promise : Promise.resolve(peerSnapshot(id, id)),
  });
  const { setWorkspaceBrowseTarget } = await import("../../lib/workspaceBrowseTarget");
  try {
    await harness.act(async () => harness.actions.setActiveTab("ws-renewal", "mesh:b/main"));
    const selected = harness.focusedTabId;
    assert(selected === "mesh:b/main", "the remote viewer is selected before workspace browsing");
    browsing = true;
    await harness.act(async () => setWorkspaceBrowseTarget({ id: "a", name: "A" }));
    await harness.act(async () => setWorkspaceBrowseTarget({ id: "b", name: "B" }));
    await harness.act(async () => setWorkspaceBrowseTarget({ id: "a", name: "A" }));
    assert(browseReads.length === 0, "all three target requests started");
    await harness.act(async () => finalA.resolve(peerSnapshot("a", "Current A workspace")));
    await harness.act(async () => {
      initialA.resolve(peerSnapshot("a", "Stale A workspace"));
      initialB.resolve(peerSnapshot("b", "Stale B workspace"));
    });
    assert(harness.inventory.browse?.workspaces[0].workspace.name === "Current A workspace", "only the latest target response contributes workspace inventory");
    assert(harness.focusedTabId === selected && harness.tabIds.includes(selected), "browsing kept the same viewer identity and selection");
    assert(harness.inventory.generals.length === 2, "browsing kept both paired Generals");
  } finally {
    await harness.act(async () => setWorkspaceBrowseTarget(null));
    await harness.unmount();
  }
});

async function resyncCase(name: string, fire: () => Promise<void> | void) {
  test(`resync: ${name} triggers exactly one mux_get_state call`, async () => {
    const harness = await mountProvider();
    const baseline = harness.muxGetStateCalls;

    await harness.act(async () => {
      await fire();
    });
    await waitUntil(() => harness.muxGetStateCalls >= baseline + 1, harness.act);
    await harness.settleMuxGetState();

    const delta = harness.muxGetStateCalls - baseline;
    assert(delta === 1, `expected exactly one resync call for "${name}", got ${delta}`);
    await harness.unmount();
  });
}

// Case 1 — stream disconnect: the explicit test seam that forces a rebind,
// standing in for a genuinely torn-down mux-state-changed subscription.
void resyncCase("stream disconnect", () => {
  const stats = (window as unknown as { __ctEvents?: { breakMuxStream?: () => void } }).__ctEvents;
  assert(typeof stats?.breakMuxStream === "function", "breakMuxStream test seam must be installed");
  stats!.breakMuxStream!();
});

// Case 2 — overflow: webEvents.ts always fires this on any overflow frame
// (ring-evicted / lagged), regardless of reason.
void resyncCase("overflow signal", () => {
  window.dispatchEvent(new Event("codeterm-refresh-mux-state"));
});

// Case 3 — boot-namespace change: production always pairs this with the
// overflow refresh event above (webEvents.ts fires both for that reason).
void resyncCase("boot-namespace change", () => {
  window.dispatchEvent(new Event("codeterm-refresh-mux-state"));
  window.dispatchEvent(new CustomEvent("codeterm-web-boot-changed", { detail: { boot: "boot-2" } }));
});

// Case 4 — a whole-daemon context rebind has its own reset lifecycle.
void resyncCase("daemon context rebind", () => {
  const switchId = 7;
  window.dispatchEvent(new CustomEvent("codeterm-peer-switch-reset", { detail: { switchId } }));
  window.dispatchEvent(new CustomEvent("codeterm-peer-switch-rebind", { detail: { switchId } }));
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
