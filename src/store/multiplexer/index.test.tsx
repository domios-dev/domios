// Run: npx tsx src/store/multiplexer/index.test.tsx
//
// codeterm#336 part B: the mux event stream must not renew itself on a fixed
// 10s timer. It rebinds only after an OBSERVED signal (stream disconnect,
// overflow, boot-namespace change, peer switch) or the explicit test seam
// below. These oracles count `mux_get_state` calls against an injected fake
// backend — never against elapsed time or rendered text.

export {};

// Enables isCodeTermFrontendTestMode() so the SUT installs the breakMuxStream
// test seam (chatTestBridge.ts's documented harness signal — env over query
// string, since this test never gives the happy-dom window a `?e2e=1` URL).
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

function bootstrapWebGlobals() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  (globalThis as { __BUILD_HASH__?: string }).__BUILD_HASH__ = "mux-renewal-test";
  (globalThis as unknown as { localStorage: FakeStorage }).localStorage = new FakeStorage();
  (globalThis as unknown as { sessionStorage: FakeStorage }).sessionStorage = new FakeStorage();
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

// A recurring-capable fake clock so "advance 60s of virtual time" never
// blocks on the real wall clock. Only patches the happy-dom `window`
// instance's timer methods — the SUT calls `window.setInterval` explicitly.
class FakeClock {
  private now = 0;
  private nextId = 1;
  private jobs = new Map<number, { due: number; delayMs: number; recurring: boolean; callback: () => void }>();
  private restore: (() => void) | null = null;

  install(target: Record<string, unknown>): void {
    const original: Record<string, unknown> = {
      setInterval: target.setInterval,
      clearInterval: target.clearInterval,
      setTimeout: target.setTimeout,
      clearTimeout: target.clearTimeout,
    };
    const schedule = (recurring: boolean) => (callback: () => void, delay = 0) => {
      const id = this.nextId++;
      const delayMs = Number(delay) || 0;
      this.jobs.set(id, { due: this.now + delayMs, delayMs, recurring, callback });
      return id as unknown as ReturnType<typeof setInterval>;
    };
    const clear = (id: number) => this.jobs.delete(id);
    for (const [key, value] of Object.entries({
      setInterval: schedule(true),
      setTimeout: schedule(false),
      clearInterval: clear,
      clearTimeout: clear,
    })) {
      Object.defineProperty(target, key, { configurable: true, writable: true, value });
    }
    this.restore = () => {
      for (const key of Object.keys(original)) {
        Object.defineProperty(target, key, { configurable: true, writable: true, value: original[key] });
      }
    };
  }

  uninstall(): void {
    this.restore?.();
    this.restore = null;
    this.jobs.clear();
  }

  /** Advance virtual time, firing every due job (recurring jobs reschedule). */
  async advance(ms: number, act: (fn: () => Promise<void>) => Promise<void>): Promise<void> {
    const deadline = this.now + ms;
    for (;;) {
      const due = [...this.jobs.entries()]
        .filter(([, job]) => job.due <= deadline)
        .sort(([, a], [, b]) => a.due - b.due)[0];
      if (!due) break;
      const [id, job] = due;
      this.now = job.due;
      if (job.recurring) {
        job.due = this.now + job.delayMs;
      } else {
        this.jobs.delete(id);
      }
      await act(async () => {
        job.callback();
        await Promise.resolve();
        await Promise.resolve();
      });
    }
    this.now = deadline;
    await act(async () => {
      await Promise.resolve();
    });
  }
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

  return { act, useLayoutEffect, createRoot, React, window };
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

interface Harness {
  root: import("react-dom/client").Root;
  act: typeof import("react").act;
  window: InstanceType<typeof import("happy-dom").Window>;
  muxGetStateCalls: number;
  unmount: () => Promise<void>;
}

/** Mounts MultiplexerProvider with an injected, permanently-healthy backend
 * and listener, waits for initial mount effects to settle, and returns a
 * running mux_get_state tally. */
async function mountProvider(): Promise<Harness> {
  (globalThis as unknown as { localStorage: FakeStorage }).localStorage.clear();
  (globalThis as unknown as { sessionStorage: FakeStorage }).sessionStorage.clear();

  const { act, createRoot, React, window } = await setupHappyDom();
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { MultiplexerProvider } = await import("./index");

  let muxGetStateCalls = 0;
  __setInvokeForTest(async <T,>(cmd: string): Promise<T> => {
    if (cmd === "mux_get_state") {
      muxGetStateCalls++;
      return sampleState("ws-renewal") as unknown as T;
    }
    if (cmd === "list_notifications") return [] as unknown as T;
    if (cmd === "ordinary_tabs_list") return [] as unknown as T;
    return undefined as unknown as T;
  });

  __setListenForTest(async () => () => {});

  const host = window.document.createElement("div");
  window.document.body.appendChild(host);
  const root = createRoot(host as unknown as Element);
  await act(async () => {
    root.render(React.createElement(MultiplexerProvider, null, null));
  });
  // Flush the microtask chain from the mount-time invoke() promises.
  await act(async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
  });

  return {
    root,
    act,
    window,
    get muxGetStateCalls() {
      return muxGetStateCalls;
    },
    unmount: async () => {
      await act(async () => root.unmount());
      __setInvokeForTest(null);
      __setListenForTest(null);
    },
  } as Harness;
}

test("healthy stream: mux_get_state count does not grow over 60s of virtual time", async () => {
  const clock = new FakeClock();
  const harness = await mountProvider();
  const baseline = harness.muxGetStateCalls;
  assert(baseline > 0, "mount should hydrate at least once");

  clock.install(harness.window as unknown as Record<string, unknown>);
  try {
    await clock.advance(60_000, harness.act);
  } finally {
    clock.uninstall();
  }

  assert(
    harness.muxGetStateCalls === baseline,
    `expected no growth in mux_get_state calls across 60s of virtual time on a healthy stream, baseline=${baseline} after=${harness.muxGetStateCalls}`,
  );
  await harness.unmount();
});

async function resyncCase(
  name: string,
  fire: () => Promise<void> | void,
) {
  test(`resync: ${name} triggers exactly one mux_get_state call`, async () => {
    const harness = await mountProvider();
    const baseline = harness.muxGetStateCalls;

    await harness.act(async () => {
      await fire();
      for (let i = 0; i < 20; i++) await Promise.resolve();
    });

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
// overflow refresh event above (webEvents.ts runFullResync fires both in the
// same call for reason "boot-changed"); this case reproduces that pairing.
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
