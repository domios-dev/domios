// Run: npx tsx src/store/multiplexer/writeGate.test.tsx

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
  saveStateSnapshot(sampleCachedState(wsId));
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
  onLayout: (setActiveWorkspace: (id: string) => void) => void,
) {
  const { act, useLayoutEffect, createRoot, React } = await setupHappyDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { MultiplexerProvider, useMultiplexer } = await import("./index");

  __setListenForTest(async () => () => {});

  function Probe() {
    const { setActiveWorkspace } = useMultiplexer();
    useLayoutEffect(() => {
      onLayout(setActiveWorkspace);
    }, [setActiveWorkspace]);
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
  (globalThis as unknown as { localStorage: FakeStorage }).localStorage.clear();
  await seedCache("ws-a");
  const { loadCachedStateSync } = await import("../../lib/stateCache");
  assert(loadCachedStateSync() !== null, "cache should round-trip in test harness");
  const { webBootHasSyncCache } = await import("./index");
  assert(webBootHasSyncCache() === true, "sync cache should be detected at boot");
});

test("webBootHasSyncCache is false with no cache", async () => {
  (globalThis as unknown as { localStorage: FakeStorage }).localStorage.clear();
  const { webBootHasSyncCache } = await import("./index");
  assert(webBootHasSyncCache() === false, "empty storage should not lock boot");
});

test("guarded mux writes stay blocked until first reconcile, then open", async () => {
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
