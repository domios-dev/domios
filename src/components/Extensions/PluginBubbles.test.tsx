// Run: npx tsx src/components/Extensions/PluginBubbles.test.tsx
// codeterm#336 part C: PluginBubbles wires isActive/cwd/tabId into the shared cache. Coalescing/gating logic itself is covered in src/lib/pluginBubbleCache.test.ts.

export {};

process.env.VITE_CODETERM_TEST = "1";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { __BUILD_HASH__?: string }).__BUILD_HASH__ = "plugin-bubbles-test";

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

/** Polls a real condition to completion; fails deterministically past a
 * bounded safety deadline instead of guessing a fixed tick count. */
async function waitUntil(condition: () => boolean, act: ActFn, maxTurns = 1000): Promise<void> {
  for (let i = 0; i < maxTurns; i++) {
    if (condition()) return;
    await act(async () => {
      await Promise.resolve();
    });
  }
  throw new Error(`waitUntil: condition not met within ${maxTurns} microtask turns`);
}

/** Drains pending microtasks — only for negative assertions where there is
 * no positive condition to poll for. */
async function drain(act: ActFn, turns = 20): Promise<void> {
  for (let i = 0; i < turns; i++) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

async function setupFixture(pluginId = "fake-git") {
  const { act, createRoot, React, window } = await setupHappyDom();
  const bridge = await import("../../lib/bridge");
  const snapshotStore = await import("../../lib/pluginSnapshotStore");
  const cache = await import("../../lib/pluginBubbleCache");
  const { PluginBubbles } = await import("./PluginBubbles");

  snapshotStore.__resetPluginSnapshotStoreForTest();
  cache.__resetPluginBubbleCacheForTest();

  bridge.__setInvokeForTest(async <T,>(cmd: string): Promise<T> => {
    if (cmd === "plugin_manager_snapshot") {
      return {
        plugins: [{ id: pluginId, display_name: "Fake", capabilities: { status_bubble: true } }],
        channels: [],
        diff: [],
        marketplace: [],
      } as unknown as T;
    }
    return undefined as unknown as T;
  });

  let calls = 0;
  cache.__setPluginBubbleFetcherForTest(async () => {
    calls++;
    return { label: "clean" };
  });

  const host = window.document.createElement("div");
  window.document.body.appendChild(host);
  const root = createRoot(host as unknown as Element);

  return {
    act,
    React,
    root,
    PluginBubbles,
    pluginId,
    get calls() {
      return calls;
    },
    teardown: () => {
      bridge.__setInvokeForTest(null);
      cache.__resetPluginBubbleCacheForTest();
      snapshotStore.__resetPluginSnapshotStoreForTest();
    },
  };
}

test("a hidden tab performs zero status-bubble provider calls", async () => {
  const fx = await setupFixture();
  await fx.act(async () => {
    fx.root.render(fx.React.createElement(fx.PluginBubbles, { cwd: "/repo", tabId: "tab-hidden", isActive: false }));
  });

  // Drains the plugin-snapshot fetch (unrelated to bubble computation); a
  // hidden tab must still never call the bubble fetcher.
  await drain(fx.act);
  assert(fx.calls === 0, `hidden tab must never compute, got ${fx.calls} call(s)`);

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("a visible tab computes exactly once", async () => {
  const fx = await setupFixture();
  await fx.act(async () => {
    fx.root.render(fx.React.createElement(fx.PluginBubbles, { cwd: "/repo", tabId: "tab-visible", isActive: true }));
  });

  await waitUntil(() => fx.calls >= 1, fx.act);
  assert(fx.calls === 1, `expected exactly one computation, got ${fx.calls}`);

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
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
