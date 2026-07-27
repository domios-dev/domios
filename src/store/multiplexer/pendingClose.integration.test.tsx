// Run: npx tsx src/store/multiplexer/pendingClose.integration.test.tsx
// Regression: failed/redundant closes must neither hide items nor resurrect them.

export {};

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
  (globalThis as { __BUILD_HASH__?: string }).__BUILD_HASH__ = "pending-close-test";
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
}

bootstrapWebGlobals();

type TestFn = () => Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

// ── authoritative backend model ────────────────────────────────────────────
interface BackendTab {
  id: string;
  title: string;
}
interface BackendWs {
  id: string;
  tabs: BackendTab[];
}

function makeSnapshot(workspaces: BackendWs[], gen: number) {
  const tabs: Record<string, unknown> = {};
  for (const w of workspaces) {
    for (const tab of w.tabs) tabs[tab.id] = tab;
  }
  return {
    workspaces: workspaces.map((w) => ({
      id: w.id,
      name: w.id,
      tabs: w.tabs.map((tab) => tab.id),
      active_tab_id: w.tabs[0]?.id ?? null,
      cwd: null,
    })),
    tabs,
    active_workspace_id: workspaces[0]?.id ?? null,
    focused_tab_id: null,
    generation: gen,
  };
}

interface MuxLike {
  workspaces: Array<{ id: string; tabs: string[] }>;
}
function hasTab(state: MuxLike, tabId: string): boolean {
  return state.workspaces.some((w) => w.tabs.includes(tabId));
}
function hasWorkspace(state: MuxLike, wsId: string): boolean {
  return state.workspaces.some((w) => w.id === wsId);
}

async function setupHappyDom() {
  const React = await import("react");
  const { act, useEffect } = React;
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
  return { act, useEffect, createRoot, React };
}

// Drain the microtask queue so awaited invoke/dispatch chains settle. Bounded
// so a broken path fails fast instead of hanging; no wall clock is consulted.
async function drain(n = 40): Promise<void> {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

interface Harness {
  root: { unmount: () => void };
  act: (cb: () => Promise<void>) => Promise<void>;
  actions: () => import("./context").MultiplexerActionsContextType;
  state: () => MuxLike;
  emitMux: (snapshot: unknown) => void;
  clear: () => void;
}

async function renderHarness(
  invokeMock: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>,
): Promise<Harness> {
  const { act, useEffect, createRoot, React } = await setupHappyDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { MultiplexerProvider, useMultiplexer, useMuxState } = await import("./index");

  __setInvokeForTest(invokeMock as never);
  const listeners: Record<string, (e: { payload: unknown }) => void> = {};
  __setListenForTest(async <T,>(event: string, handler: (e: { payload: T }) => void) => {
    listeners[event] = handler as (e: { payload: unknown }) => void;
    return () => {
      delete listeners[event];
    };
  });

  const stateHolder: { current: MuxLike | null } = { current: null };

  let actionsRef: import("./context").MultiplexerActionsContextType | null = null;
  function Probe() {
    const actions = useMultiplexer();
    const state = useMuxState();
    actionsRef = actions;
    useEffect(() => {
      stateHolder.current = state as unknown as MuxLike;
    }, [state]);
    return null;
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(React.createElement(MultiplexerProvider, null, React.createElement(Probe)));
    await drain(); // let the initial mux_get_state reconcile settle
  });

  return {
    root,
    act,
    actions: () => {
      assert(actionsRef, "actions not bound");
      return actionsRef!;
    },
    state: () => {
      assert(stateHolder.current, "state not captured");
      return stateHolder.current!;
    },
    emitMux: (snapshot: unknown) => {
      assert(listeners["mux-state-changed"], "mux-state-changed listener not bound");
      listeners["mux-state-changed"]({ payload: snapshot });
    },
    clear: () => {
      __setInvokeForTest(null);
      __setListenForTest(null);
    },
  };
}

test("failed closeTab restores the tab from authoritative state", async () => {
  const backend: BackendWs[] = [
    { id: "w1", tabs: [{ id: "tab-A", title: "A" }, { id: "tab-B", title: "B" }] },
  ];
  let gen = 10;
  const invokeMock = async <T,>(cmd: string): Promise<T> => {
    if (cmd === "mux_get_state") return makeSnapshot(backend, ++gen) as T;
    if (cmd === "list_notifications") return [] as T;
    // Backend rejects the close: the tab keeps living.
    if (cmd === "mux_close_tab") throw new Error("close rejected");
    return {} as T;
  };

  const h = await renderHarness(invokeMock as never);
  assert(hasTab(h.state(), "tab-B"), "tab B present after initial reconcile");

  // No mux events or timers fire in-test, so the ONLY way B returns after the
  // optimistic removal is the close-failure's own authoritative refresh.
  await h.act(async () => {
    h.actions().closeTab("w1", "tab-B");
    await drain(); // failed close -> restore -> authoritative refresh
  });

  assert(hasTab(h.state(), "tab-B"), "tab B must reappear from authoritative state after a failed close");
  await h.act(async () => h.root.unmount());
  h.clear();
});

test("failed closeWorkspace restores the workspace from authoritative state", async () => {
  const backend: BackendWs[] = [
    { id: "w1", tabs: [{ id: "tab-A", title: "A" }] },
    { id: "w2", tabs: [{ id: "tab-X", title: "X" }] },
  ];
  let gen = 10;
  const invokeMock = async <T,>(cmd: string): Promise<T> => {
    if (cmd === "mux_get_state") return makeSnapshot(backend, ++gen) as T;
    if (cmd === "list_notifications") return [] as T;
    if (cmd === "mux_close_workspace") throw new Error("close rejected");
    return {} as T;
  };

  const h = await renderHarness(invokeMock as never);
  assert(hasWorkspace(h.state(), "w1"), "workspace w1 present after initial reconcile");

  await h.act(async () => {
    h.actions().closeWorkspace("w1");
    await drain(); // failed close -> restore -> authoritative refresh
  });

  assert(hasWorkspace(h.state(), "w1"), "workspace w1 must reappear from authoritative state after a failed close");
  await h.act(async () => h.root.unmount());
  h.clear();
});

test("successful closeWorkspace stays removed (authoritative removal sticks)", async () => {
  let backend: BackendWs[] = [
    { id: "w1", tabs: [{ id: "tab-A", title: "A" }] },
    { id: "w2", tabs: [{ id: "tab-X", title: "X" }] },
  ];
  let gen = 10;
  const invokeMock = async <T,>(cmd: string): Promise<T> => {
    if (cmd === "mux_get_state") return makeSnapshot(backend, ++gen) as T;
    if (cmd === "list_notifications") return [] as T;
    if (cmd === "mux_close_workspace") {
      backend = backend.filter((w) => w.id !== "w1"); // backend actually removes it
      return {} as T;
    }
    return {} as T;
  };

  const h = await renderHarness(invokeMock as never);
  assert(hasWorkspace(h.state(), "w1"), "workspace w1 present after initial reconcile");

  await h.act(async () => {
    h.actions().closeWorkspace("w1");
    await drain(); // success -> authoritative refresh (backend no longer has w1)
  });

  // The tombstone must have self-cleared and the workspace must NOT reappear.
  assert(!hasWorkspace(h.state(), "w1"), "successfully closed workspace must not reappear");
  assert(hasWorkspace(h.state(), "w2"), "sibling workspace remains");
  await h.act(async () => h.root.unmount());
  h.clear();
});

test("redundant tab-close failure after tab-close success keeps a delayed pre-close snapshot masked", async () => {
  const backend: BackendWs[] = [
    { id: "w1", tabs: [{ id: "tab-A", title: "A" }, { id: "tab-B", title: "B" }] },
  ];
  let gen = 10;
  let hold = false;
  let closeCalls = 0;
  const invokeMock = async <T,>(cmd: string): Promise<T> => {
    if (cmd === "mux_get_state") {
      if (hold) return new Promise<T>(() => {}); // hold the authoritative post-close refresh
      return makeSnapshot(backend, ++gen) as T;
    }
    if (cmd === "list_notifications") return [] as T;
    if (cmd === "mux_close_tab") {
      closeCalls += 1;
      if (closeCalls === 1) {
        backend[0].tabs = backend[0].tabs.filter((tab) => tab.id !== "tab-B");
        return {} as T;
      }
      throw new Error("tab already gone");
    }
    return {} as T;
  };

  const h = await renderHarness(invokeMock as never);
  assert(hasTab(h.state(), "tab-B"), "tab B present after initial reconcile");
  hold = true; // from here the authoritative omission never arrives

  await h.act(async () => {
    h.actions().closeTab("w1", "tab-B");
    await drain();
  });

  // Deliver a delayed PRE-close snapshot: higher generation, still lists B.
  await h.act(async () => {
    h.emitMux(makeSnapshot(backend, 999));
    await drain();
  });

  assert(
    !hasTab(h.state(), "tab-B"),
    "redundant tab-close failure must not clear the tombstone; delayed pre-close snapshot stays masked",
  );
  await h.act(async () => h.root.unmount());
  h.clear();
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
