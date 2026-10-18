// Run: npx tsx src/store/multiplexer/writeGate.test.tsx

export {};
import type { ExtendedState } from "./tree";
import { configureDaemonTransport } from "../../lib/daemonBootstrap";
import type { FrontendCacheChanges, FrontendCacheCommit, FrontendCacheManifest, FrontendCacheRecord, FrontendCacheSnapshot, FrontendCacheStorage } from "../../lib/frontendCacheRecords";

class FakeCacheStorage implements FrontendCacheStorage {
  private readonly scopes = new Map<string, { manifest: FrontendCacheManifest; records: Map<string, FrontendCacheRecord> }>();
  private readonly commits: Array<{ scope: string; changes: FrontendCacheChanges }> = [];
  private readGate?: { entered: () => void; finished: () => void; wait: Promise<void> };

  committedChanges(scope: string): FrontendCacheChanges[] {
    return this.commits.filter((commit) => commit.scope === scope).map((commit) => structuredClone(commit.changes));
  }

  holdNextRead(): { entered: Promise<void>; finished: Promise<void>; release: () => void } {
    let entered!: () => void;
    let finished!: () => void;
    let release!: () => void;
    const enteredPromise = new Promise<void>((resolve) => { entered = resolve; });
    const finishedPromise = new Promise<void>((resolve) => { finished = resolve; });
    const wait = new Promise<void>((resolve) => { release = resolve; });
    this.readGate = { entered, finished, wait };
    return { entered: enteredPromise, finished: finishedPromise, release };
  }

  async read(scope: string): Promise<FrontendCacheSnapshot | null> {
    const gate = this.readGate;
    this.readGate = undefined;
    if (gate) { gate.entered(); await gate.wait; gate.finished(); }
    const value = this.scopes.get(scope);
    return value ? structuredClone({ manifest: value.manifest, records: [...value.records.values()] }) : null;
  }

  async commit(scope: string, expectedRevision: number, manifest: FrontendCacheManifest, changes: FrontendCacheChanges): Promise<FrontendCacheCommit> {
    const current = this.scopes.get(scope);
    if ((current?.manifest.revision ?? 0) !== expectedRevision) return "conflict";
    const records = new Map(current?.records ?? []);
    for (const row of changes.deletes) records.delete(`${row.kind}\u0000${row.id}`);
    for (const row of changes.upserts) records.set(`${row.kind}\u0000${row.id}`, structuredClone(row));
    assert(records.size === manifest.recordCount, "transaction commits a complete cache generation");
    this.scopes.set(scope, { manifest: structuredClone(manifest), records });
    this.commits.push({ scope, changes: structuredClone(changes) });
    return "committed";
  }

  async clear(scope: string, expectedRevision?: number): Promise<boolean> {
    const current = this.scopes.get(scope);
    if (expectedRevision !== undefined && (current?.manifest.revision ?? 0) !== expectedRevision) return false;
    return this.scopes.delete(scope);
  }
}

const cacheStorage = new FakeCacheStorage();

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

function bootstrapWebGlobals() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  (globalThis as { __BUILD_HASH__?: string }).__BUILD_HASH__ = "write-gate-test";
  (globalThis as unknown as { window: unknown }).window = {
    location: { protocol: "http:", host: "localhost:7686" },
  };
  (globalThis as unknown as { localStorage: FakeStorage }).localStorage = new FakeStorage();
  (globalThis as unknown as { sessionStorage: FakeStorage }).sessionStorage = new FakeStorage();
  if (typeof (globalThis as { requestAnimationFrame?: unknown }).requestAnimationFrame !== "function") {
    (globalThis as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame = (
      cb: FrameRequestCallback,
    ) => {
      cb(0);
      return 0;
    };
    (globalThis as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = () => {};
  }
  configureDaemonTransport({ origin: "http://localhost:7686", token: "write-gate-bootstrap" });
}

bootstrapWebGlobals();

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function sampleCachedState(wsId: string) {
  return {
    workspaces: [{ id: wsId, name: "Cached", tabs: [], active_tab_id: null, cwd: null }],
    tabs: {},
    active_workspace_id: wsId,
    focused_tab_id: null,
    generation: 1,
  };
}

async function seedCache(wsId: string) {
  const { saveStateSnapshot } = await import("../../lib/stateCache");
  await saveStateSnapshot(sampleCachedState(wsId));
}

async function setupHappyDom() {
  const React = await import("react");
  const { act, useLayoutEffect } = React;
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

  return { act, useLayoutEffect, createRoot, React };
}

async function renderProvider(
  onLayout: (setActiveWorkspace: (id: string) => void, state: ExtendedState) => void,
  onState?: (state: ExtendedState) => void,
  listenForTest?: <T>(event: string, handler: (e: { payload: T }) => void) => Promise<() => void>,
) {
  const { act, useLayoutEffect, createRoot, React } = await setupHappyDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { MultiplexerProvider, useMultiplexer, useMuxState } = await import("./index");

  __setListenForTest(listenForTest ?? (async () => () => {}));

  function Probe() {
    const { setActiveWorkspace } = useMultiplexer();
    const state = useMuxState();
    useLayoutEffect(() => {
      onLayout(setActiveWorkspace, state);
    }, [setActiveWorkspace]);
    useLayoutEffect(() => { onState?.(state); }, [state]);
    return null;
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      React.createElement(
        MultiplexerProvider,
        null,
        React.createElement(Probe),
      ),
    );
  });
  return { root, act, __setInvokeForTest, __setListenForTest };
}

test("webBootHasSyncCache is true when sync cache exists", async () => {
  const { setFrontendCacheStorageForTests } = await import("../../lib/frontendCacheRecords");
  setFrontendCacheStorageForTests(cacheStorage);
  configureDaemonTransport({ origin: "http://localhost:7686", token: "write-gate-seed" });
  (globalThis as unknown as { localStorage: FakeStorage }).localStorage.clear();
  await seedCache("ws-a");
  const { loadCachedStateSync } = await import("../../lib/stateCache");
  assert(loadCachedStateSync() !== null, "cache should round-trip in test harness");
  const { webBootHasSyncCache } = await import("./index");
  assert(webBootHasSyncCache() === true, "sync cache should be detected at boot");
});

test("webBootHasSyncCache is false with no cache", async () => {
  const { setFrontendCacheStorageForTests } = await import("../../lib/frontendCacheRecords");
  setFrontendCacheStorageForTests(cacheStorage);
  configureDaemonTransport({ origin: "http://localhost:7686", token: "write-gate-empty" });
  (globalThis as unknown as { localStorage: FakeStorage }).localStorage.clear();
  const { webBootHasSyncCache } = await import("./index");
  assert(webBootHasSyncCache() === false, "empty storage should not lock boot");
});

test("the first cached render selects the restored chat before a daemon read completes", async () => {
  const { setFrontendCacheStorageForTests } = await import("../../lib/frontendCacheRecords");
  setFrontendCacheStorageForTests(cacheStorage);
  configureDaemonTransport({ origin: "http://localhost:7686", token: "write-gate-first-render" });
  const { saveStateSnapshot } = await import("../../lib/stateCache");
  const { __setInvokeForTest } = await import("../../lib/bridge");
  const snapshot = { ...sampleCachedState("restored"), workspaces: [
    { id: "restored", name: "Restored", tabs: ["chat"], active_tab_id: "chat", cwd: null },
  ], tabs: { chat: { id: "chat", title: "Chat", pty_id: null, cwd: null, tab_type: "Terminal" as const } },
    tab_statuses: { chat: "Waiting" as const } };
  saveStateSnapshot(snapshot);
  __setInvokeForTest(async <T,>(cmd: string): Promise<T> => {
    if (cmd === "mux_get_state") return new Promise<T>(() => {});
    return [] as T;
  });
  let firstState: ExtendedState | undefined;
  const { root, act, __setListenForTest } = await renderProvider((_select, state) => { firstState ??= state; });
  assert(firstState?.active_workspace_id === "restored", "first render restores the workspace");
  assert(firstState.activeTabIds.restored === "chat", "first render selects the chat instead of an empty plan view");
  assert(firstState.focused_tab_id === "chat", "first render focuses the restored chat");
  assert(firstState.tabStatuses.chat === "Waiting", "first cached render presents the retained tab status");
  await act(async () => root.unmount());
  __setInvokeForTest(null);
  __setListenForTest(null);
});

test("async cache hydration presents retained status while live state is pending", async () => {
  const { frontendCacheScope, FrontendCacheRepository, setFrontendCacheStorageForTests } = await import("../../lib/frontendCacheRecords");
  setFrontendCacheStorageForTests(cacheStorage);
  configureDaemonTransport({ origin: "http://localhost:7686", token: "write-gate-async-status" });
  const scope = frontendCacheScope("codeterm:state-cache");
  assert(scope, "web cache scope exists");
  const workspace = { id: "cached-status", name: "Cached", tabs: ["cached-tab"], active_tab_id: "cached-tab", cwd: null };
  const tab = { id: "cached-tab", title: "Cached tab", pty_id: null, cwd: null, tab_type: "Terminal" as const };
  await new FrontendCacheRepository(cacheStorage).write(scope, [
    { kind: "workspace", id: workspace.id, value: { position: 0, workspace } },
    { kind: "tab", id: tab.id, value: tab },
    { kind: "tab-status", id: tab.id, value: "Waiting" },
    { kind: "selection", id: "current", value: { active_workspace_id: workspace.id, focused_tab_id: tab.id, activeTabIds: { [workspace.id]: tab.id } } },
  ], {
    schemaVersion: 2,
    buildHash: "write-gate-test",
    generation: 8,
    counts: { workspace: 1, tab: 1, "tab-status": 1, selection: 1 },
  });

  const { __setInvokeForTest } = await import("../../lib/bridge");
  __setInvokeForTest(async <T,>(cmd: string): Promise<T> => {
    if (cmd === "mux_get_state") return new Promise<T>(() => {});
    if (cmd === "list_notifications") return [] as T;
    return [] as T;
  });
  let resolveCachedStatus!: () => void;
  const cachedStatusSeen = new Promise<void>((resolve) => { resolveCachedStatus = resolve; });
  let cachedState: ExtendedState | undefined;
  const { root, act, __setInvokeForTest: clearInvoke, __setListenForTest } = await renderProvider(() => {}, (state) => {
    if (state.tabStatuses["cached-tab"] === "Waiting") {
      cachedState = state;
      resolveCachedStatus();
    }
  });
  try {
    await act(async () => { await cachedStatusSeen; });
    assert(cachedState?.tabStatuses["cached-tab"] === "Waiting", "cache hydration presents retained status before live reconcile");
  } finally {
    await act(async () => root.unmount());
    clearInvoke(null);
    __setListenForTest(null);
  }
});

test("mux status-only updates persist one status row and identical status writes nothing", async () => {
  const { frontendCacheScope, setFrontendCacheStorageForTests } = await import("../../lib/frontendCacheRecords");
  setFrontendCacheStorageForTests(cacheStorage);
  configureDaemonTransport({ origin: "http://localhost:7686", token: "write-gate-status-projection" });
  const scope = frontendCacheScope("codeterm:state-cache");
  assert(scope, "web cache scope exists");

  const handlers = new Map<string, Array<(event: { payload: unknown }) => void>>();
  const { __setInvokeForTest } = await import("../../lib/bridge");
  let resolveMuxListeners!: () => void;
  const muxListenersBound = new Promise<void>((resolve) => { resolveMuxListeners = resolve; });
  let resolveStatusListener!: () => void;
  const statusListenerBound = new Promise<void>((resolve) => { resolveStatusListener = resolve; });
  const listenForStatusTest = async <T,>(event: string, handler: (e: { payload: T }) => void): Promise<() => void> => {
    const registered = handlers.get(event) ?? [];
    registered.push(handler as (event: { payload: unknown }) => void);
    handlers.set(event, registered);
    if (event === "mux-state-changed" && registered.length === 2) resolveMuxListeners();
    if (event === "tab-status-changed") resolveStatusListener();
    return () => {};
  };
  const liveSnapshot = {
    ...sampleCachedState("ws-status"),
    tabs: { "status-tab": { id: "status-tab", title: "Status", pty_id: null, cwd: null, tab_type: "Terminal" as const } },
    workspaces: [{ id: "ws-status", name: "Status", tabs: ["status-tab"], active_tab_id: "status-tab", cwd: null }],
    tab_statuses: {},
  };
  __setInvokeForTest(async <T,>(cmd: string): Promise<T> => {
    if (cmd === "mux_get_state") return new Promise<T>(() => {});
    if (cmd === "list_notifications") return [] as T;
    if (cmd === "ordinary_tabs_list") return [] as T;
    if (cmd === "mesh_list_peers") return { paired: [] } as T;
    return [] as T;
  });

  let renderedState: ExtendedState | undefined;
  const { root, act, __setInvokeForTest: clearInvoke, __setListenForTest: clearListen } = await renderProvider(() => {}, (state) => {
    renderedState = state;
  }, listenForStatusTest);
  try {
    await muxListenersBound;
    await statusListenerBound;
    const muxHandlers = handlers.get("mux-state-changed") ?? [];
    const acceptMuxSnapshot = muxHandlers[muxHandlers.length - 1];
    assert(acceptMuxSnapshot, "authoritative mux listener is bound");
    await act(async () => {
      acceptMuxSnapshot({ payload: liveSnapshot });
    });
    assert(renderedState?.generation === 1, "authoritative mux snapshot is accepted before status-only update");
    const { loadCachedStateAsync } = await import("../../lib/stateCache");
    await loadCachedStateAsync();
    const commitsBeforeStatus = cacheStorage.committedChanges(scope).length;
    const emitStatus = () => {
      for (const handler of handlers.get("tab-status-changed") ?? []) {
        handler({ payload: { tab_id: "status-tab", status: "Working" } });
      }
    };
    assert((handlers.get("tab-status-changed") ?? []).length > 0, "live status listener is bound");
    await act(async () => { emitStatus(); });
    assert(renderedState?.tabStatuses["status-tab"] === "Working", "status-only event updates the live mux projection");
    const afterStatus = await loadCachedStateAsync();
    assert(afterStatus?.tab_statuses?.["status-tab"] === "Working", "accepted status-only event reaches persisted cache state");
    const statusCommits = cacheStorage.committedChanges(scope).slice(commitsBeforeStatus);
    assert(statusCommits.length === 1, "one status update makes one cache transaction");
    assert(statusCommits[0].upserts.length === 1 && statusCommits[0].upserts[0].kind === "tab-status" &&
      statusCommits[0].upserts[0].id === "status-tab", "only the affected status record is upserted");
    assert(statusCommits[0].deletes.length === 0, "status update deletes no records");

    await act(async () => { emitStatus(); });
    await loadCachedStateAsync();
    assert(cacheStorage.committedChanges(scope).length === commitsBeforeStatus + 1,
      "an identical semantic status refresh causes no additional cache transaction");
  } finally {
    await act(async () => root.unmount());
    clearInvoke(null);
    clearListen(null);
  }
});

test("guarded mux writes stay blocked until first reconcile, then open", async () => {
  const { setFrontendCacheStorageForTests } = await import("../../lib/frontendCacheRecords");
  setFrontendCacheStorageForTests(cacheStorage);
  configureDaemonTransport({ origin: "http://localhost:7686", token: "write-gate-guard" });
  (globalThis as unknown as { localStorage: FakeStorage }).localStorage.clear();
  await seedCache("ws-cached");

  const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
  let resolveReconcile!: (state: ReturnType<typeof sampleCachedState>) => void;
  const reconcilePending = new Promise<ReturnType<typeof sampleCachedState>>((resolve) => {
    resolveReconcile = resolve;
  });

  const { __setInvokeForTest } = await import("../../lib/bridge");
  __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    if (cmd === "mux_get_state") {
      return reconcilePending as Promise<T>;
    }
    if (cmd === "list_notifications") {
      return [] as T;
    }
    if (cmd === "ordinary_tabs_list") return [] as T;
    if (cmd === "mesh_list_peers") return { paired: [] } as T;
    calls.push({ cmd, args });
    return {} as T;
  });

  let layoutSetActiveWorkspace: ((id: string) => void) | undefined;
  const { root, act, __setInvokeForTest: clearInvoke, __setListenForTest } = await renderProvider(
    (setActiveWorkspace) => {
      layoutSetActiveWorkspace = setActiveWorkspace;
      setActiveWorkspace("ws-cached");
    },
  );

  assert(typeof layoutSetActiveWorkspace === "function", "probe should expose setActiveWorkspace");
  const setActiveWorkspaceAfterMount = layoutSetActiveWorkspace;
  const blockedCalls = calls.filter((c) => c.cmd === "mux_set_active_workspace");
  assert(
    blockedCalls.length === 0,
    "layout-phase setActiveWorkspace must not invoke mux_set_active_workspace before reconcile",
  );

  resolveReconcile(sampleCachedState("ws-live"));
  await act(async () => {
    await reconcilePending;
  });

  await act(async () => {
    setActiveWorkspaceAfterMount("ws-live");
  });

  const afterReconcile = calls.filter((c) => c.cmd === "mux_set_active_workspace");
  assert(
    afterReconcile.some((c) => c.args?.workspaceId === "ws-live"),
    "setActiveWorkspace should invoke mux_set_active_workspace after reconcile unlocks the gate",
  );

  await act(async () => root.unmount());
  clearInvoke(null);
  __setListenForTest(null);
});

test("late async hydration cannot replace a live reconcile", async () => {
  const { frontendCacheScope, FrontendCacheRepository, setFrontendCacheStorageForTests } = await import("../../lib/frontendCacheRecords");
  setFrontendCacheStorageForTests(cacheStorage);
  configureDaemonTransport({ origin: "http://localhost:7686", token: "write-gate-late-hydration" });
  const scope = frontendCacheScope("codeterm:state-cache");
  assert(scope, "web cache scope exists");
  const workspace = { id: "cached", name: "Cached", tabs: [], active_tab_id: null, cwd: null };
  const records: FrontendCacheRecord[] = [
    { kind: "workspace", id: "cached", value: { position: 0, workspace } },
    { kind: "tab-status", id: "live-tab", value: "Waiting" },
    { kind: "selection", id: "current", value: { active_workspace_id: "cached", focused_tab_id: null, activeTabIds: {} } },
  ];
  await new FrontendCacheRepository(cacheStorage).write(scope, records, {
    schemaVersion: 2,
    buildHash: "write-gate-test",
    generation: 8,
    counts: { workspace: 1, "tab-status": 1, selection: 1 },
  });

  const readGate = cacheStorage.holdNextRead();
  const { __setInvokeForTest } = await import("../../lib/bridge");
  __setInvokeForTest(async <T,>(cmd: string): Promise<T> => {
    if (cmd === "mux_get_state") return { ...sampleCachedState("live"), tab_statuses: { "live-tab": "Idle" } } as T;
    if (cmd === "list_notifications") return [] as T;
    if (cmd === "ordinary_tabs_list") return [] as T;
    if (cmd === "mesh_list_peers") return { paired: [] } as T;
    return {} as T;
  });
  let liveState: ExtendedState | undefined;
  let resolveLive!: () => void;
  const liveSeen = new Promise<void>((resolve) => { resolveLive = resolve; });
  const { root, act, __setInvokeForTest: clearInvoke, __setListenForTest } = await renderProvider(() => {}, (state) => {
    if (state.active_workspace_id === "live") { liveState = state; resolveLive(); }
  });
  try {
    await readGate.entered;
    await act(async () => { await liveSeen; });
    assert(liveState?.active_workspace_id === "live", "accepted backend state reaches the UI while cache read is pending");
    assert(liveState?.tabStatuses["live-tab"] === "Idle", "accepted live status reaches the UI before cache read finishes");
    await act(async () => { readGate.release(); await readGate.finished; });
    assert(liveState?.active_workspace_id === "live", "late cache hydration is ignored after live reconcile");
    assert(liveState?.tabStatuses["live-tab"] === "Idle", "late cached status cannot replace live authority");
  } finally {
    await act(async () => root.unmount());
    clearInvoke(null);
    __setListenForTest(null);
  }
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
