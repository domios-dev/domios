// Run: npx tsx src/store/multiplexer/pendingClose.integration.test.tsx
//
// Regression: a backend close that FAILS must not leave the optimistically
// removed tab/workspace invisible. closePane/closeWorkspace must restore it
// from authoritative state (mirroring closeTab). Waits are condition-based on
// observed store renders — no timers/sleeps.

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
  pane_id: string;
}
interface BackendWs {
  id: string;
  tabs: BackendTab[];
}

function makeSnapshot(workspaces: BackendWs[], gen: number) {
  const panes: Record<string, unknown> = {};
  for (const w of workspaces) {
    for (const t of w.tabs) panes[t.pane_id] = { id: t.pane_id, title: t.id };
  }
  return {
    workspaces: workspaces.map((w) => ({
      id: w.id,
      name: w.id,
      tabs: w.tabs.map((t) => ({ id: t.id, pane_id: t.pane_id })),
      active_tab_id: w.tabs[0]?.id ?? null,
      cwd: null,
    })),
    panes,
    active_workspace_id: workspaces[0]?.id ?? null,
    focused_pane_id: null,
    generation: gen,
  };
}

interface MuxLike {
  workspaces: Array<{ id: string; tabs: Array<{ id: string; pane_id: string }> }>;
}
function hasTab(state: MuxLike, tabId: string): boolean {
  return state.workspaces.some((w) => w.tabs.some((t) => t.id === tabId));
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
// (never open-ended) so a broken path fails fast instead of hanging; no wall
// clock is consulted — this is queue draining, not a timed settle.
async function drain(n = 40): Promise<void> {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

interface Harness {
  root: { unmount: () => void };
  act: (cb: () => Promise<void>) => Promise<void>;
  actions: () => import("./context").MultiplexerActionsContextType;
  state: () => MuxLike;
  clear: () => void;
}

async function renderHarness(
  invokeMock: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>,
): Promise<Harness> {
  const { act, useEffect, createRoot, React } = await setupHappyDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { MultiplexerProvider, useMultiplexer, useMuxState } = await import("./index");

  __setInvokeForTest(invokeMock as never);
  __setListenForTest(async () => () => {});

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
    clear: () => {
      __setInvokeForTest(null);
      __setListenForTest(null);
    },
  };
}

test("failed closePane restores the tab from authoritative state", async () => {
  const backend: BackendWs[] = [
    { id: "w1", tabs: [{ id: "A", pane_id: "pane-A" }, { id: "B", pane_id: "pane-B" }] },
  ];
  let gen = 10;
  const invokeMock = async <T,>(cmd: string): Promise<T> => {
    if (cmd === "mux_get_state") return makeSnapshot(backend, ++gen) as T;
    if (cmd === "list_notifications") return [] as T;
    // Backend rejects the close: the pane keeps living.
    if (cmd === "mux_close_pane" || cmd === "mux_close_tab") throw new Error("close rejected");
    return {} as T;
  };

  const h = await renderHarness(invokeMock as never);
  assert(hasTab(h.state(), "B"), "tab B present after initial reconcile");

  // No mux events or timers fire in-test, so the ONLY way B returns after the
  // optimistic removal is the close-failure's own authoritative refresh.
  await h.act(async () => {
    h.actions().closePane("pane-B");
    await drain(); // failed close -> restore -> authoritative refresh
  });

  assert(hasTab(h.state(), "B"), "tab B must reappear from authoritative state after a failed close");
  await h.act(async () => h.root.unmount());
  h.clear();
});

test("failed closeWorkspace restores the workspace from authoritative state", async () => {
  const backend: BackendWs[] = [
    { id: "w1", tabs: [{ id: "A", pane_id: "pane-A" }] },
    { id: "w2", tabs: [{ id: "X", pane_id: "pane-X" }] },
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
    { id: "w1", tabs: [{ id: "A", pane_id: "pane-A" }] },
    { id: "w2", tabs: [{ id: "X", pane_id: "pane-X" }] },
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
