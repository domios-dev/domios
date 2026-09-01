// Run: npx tsx src/store/multiplexer/index.test.tsx
// codeterm#336 part B: mux_get_state call counts against an injected fake, gated on observed signals only.

export {};

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
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

/** `mux_get_state` responses stay pending until explicitly resolved; calls
 * are counted synchronously at invocation, before any settling. */
async function mountProvider() {
  (globalThis as unknown as { localStorage: FakeStorage }).localStorage.clear();
  (globalThis as unknown as { sessionStorage: FakeStorage }).sessionStorage.clear();

  const { act, createRoot, React, window } = await setupHappyDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { MultiplexerProvider } = await import("./index");

  let muxGetStateCalls = 0;
  const pendingMuxGetState: Array<Deferred<unknown>> = [];
  __setInvokeForTest(async <T,>(cmd: string): Promise<T> => {
    if (cmd === "mux_get_state") {
      muxGetStateCalls++;
      const d = deferred<unknown>();
      pendingMuxGetState.push(d);
      return d.promise as Promise<T>;
    }
    if (cmd === "list_notifications") return [] as unknown as T;
    if (cmd === "ordinary_tabs_list") return [] as unknown as T;
    return undefined as unknown as T;
  });
  __setListenForTest(async () => () => {});

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
    root.render(React.createElement(MultiplexerProvider, null, null));
  });

  const settleMuxGetState = async () => {
    const pending = pendingMuxGetState.splice(0, pendingMuxGetState.length);
    if (pending.length === 0) return;
    await act(async () => {
      for (const d of pending) d.resolve(sampleState("ws-renewal"));
      await Promise.all(pending.map((d) => d.promise));
    });
  };
  await settleMuxGetState();
  // The listener-bind promise resolving is a real, observable settle point
  // for the mount-time effect chain (bridge.ts's `listen` .then()).
  await waitUntil(
    () => Boolean((window as unknown as { __ctEvents?: { reconnects: number } }).__ctEvents?.reconnects),
    act,
  );

  return {
    act,
    window,
    get muxGetStateCalls() {
      return muxGetStateCalls;
    },
    get intervalCalls() {
      return intervalCalls;
    },
    settleMuxGetState,
    unmount: async () => {
      await act(async () => root.unmount());
      __setInvokeForTest(null);
      __setListenForTest(null);
    },
  };
}

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

// Case 4 — peer switch: WebPeerPicker's reset+rebind flow.
void resyncCase("peer switch", () => {
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
