// Run: npx tsx src/components/Extensions/PluginBubbles.test.tsx
//
// codeterm#336 part C: PluginBubbles must gate on the active/visible tab,
// share one computation across every tab pointed at the same repository, and
// recompute only on an explicit invalidation — never on elapsed time alone.
// Oracles count fetcher invocations against an injected fake; nothing here
// asserts rendered text or waits on a real timer.

export {};

function bootstrapGlobals() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  (globalThis as { __BUILD_HASH__?: string }).__BUILD_HASH__ = "plugin-bubbles-test";
}
bootstrapGlobals();

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

/** Flush the microtask chain (fake-fetcher resolution -> setState -> effect
 * re-run) without waiting on any real timer. */
async function flush(act: ActFn, ticks = 20): Promise<void> {
  await act(async () => {
    for (let i = 0; i < ticks; i++) await Promise.resolve();
  });
}

/** Sets up a fresh happy-dom environment, a single fake status-bubble
 * plugin (declared self-invalidating, so no safety-poll timer is ever
 * armed), and an injected fetcher that just counts invocations. */
async function setupFixture(pluginId = "fake-git") {
  const { act, createRoot, React, window } = await setupHappyDom();
  const bridge = await import("../../lib/bridge");
  const snapshotStore = await import("../../lib/pluginSnapshotStore");
  const cache = await import("../../lib/pluginBubbleCache");
  const { PluginBubbles } = await import("./PluginBubbles");

  snapshotStore.__resetPluginSnapshotStoreForTest();
  cache.__resetPluginBubbleCacheForTest();
  cache.__setSelfInvalidatingPluginsForTest([pluginId]);

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

  const fixture = {
    act,
    createRoot,
    React,
    window,
    PluginBubbles,
    pluginId,
    calls: 0,
    teardown: () => {
      bridge.__setInvokeForTest(null);
      cache.__resetPluginBubbleCacheForTest();
      snapshotStore.__resetPluginSnapshotStoreForTest();
    },
  };

  cache.__setPluginBubbleFetcherForTest(async () => {
    fixture.calls++;
    return { label: "clean", tone: "default" as const };
  });

  return fixture;
}

function mount(
  fixture: Awaited<ReturnType<typeof setupFixture>>,
  props: { cwd: string | null; tabId: string; isActive: boolean },
) {
  const host = fixture.window.document.createElement("div");
  fixture.window.document.body.appendChild(host);
  const root = fixture.createRoot(host as unknown as Element);
  return { root, render: () => fixture.React.createElement(fixture.PluginBubbles, props) };
}

test("a hidden mounted tab performs zero status-bubble provider calls", async () => {
  const fixture = await setupFixture();
  const { root, render } = mount(fixture, { cwd: "/repo/hidden", tabId: "tab-hidden", isActive: false });

  await fixture.act(async () => {
    root.render(render());
  });
  await flush(fixture.act);

  assert(fixture.calls === 0, `hidden tab must never compute, got ${fixture.calls} call(s)`);

  // No timer is ever armed for an inactive subscriber, so repeated flushing
  // (standing in for "time passing") cannot surface a hidden background poll.
  await flush(fixture.act);
  await flush(fixture.act);
  assert(fixture.calls === 0, `hidden tab must stay at zero after settling, got ${fixture.calls} call(s)`);

  await fixture.act(async () => root.unmount());
  fixture.teardown();
});

test("a visible tab computes once, once more per invalidation, never from time alone", async () => {
  const fixture = await setupFixture();
  const { root, render } = mount(fixture, { cwd: "/repo/visible", tabId: "tab-visible", isActive: true });

  await fixture.act(async () => {
    root.render(render());
  });
  await flush(fixture.act);
  assert(fixture.calls === 1, `expected exactly one initial computation, got ${fixture.calls}`);

  const cache = await import("../../lib/pluginBubbleCache");
  await fixture.act(async () => {
    cache.invalidatePluginBubble(fixture.pluginId, "/repo/visible");
  });
  await flush(fixture.act);
  assert(fixture.calls === 2, `expected exactly one more computation after invalidation, got ${fixture.calls}`);

  // Self-invalidating plugin: no safety timer exists, so flushing further
  // ("time passing" with no event) must not produce another computation.
  await flush(fixture.act);
  await flush(fixture.act);
  assert(fixture.calls === 2, `expected no further computation without an event, got ${fixture.calls}`);

  await fixture.act(async () => root.unmount());
  fixture.teardown();
});

test("ten tabs sharing a repository coalesce to one computation per invalidation", async () => {
  const fixture = await setupFixture();
  const cwd = "/repo/shared";
  const roots: Array<ReturnType<typeof mount>["root"]> = [];

  for (let i = 0; i < 10; i++) {
    const { root, render } = mount(fixture, { cwd, tabId: `tab-${i}`, isActive: i === 0 });
    await fixture.act(async () => {
      root.render(render());
    });
    roots.push(root);
  }
  await flush(fixture.act);

  assert(fixture.calls === 1, `only the one visible tab should have computed initially, got ${fixture.calls}`);

  const baseline = fixture.calls;
  const cache = await import("../../lib/pluginBubbleCache");
  await fixture.act(async () => {
    cache.invalidatePluginBubble(fixture.pluginId, cwd);
  });
  await flush(fixture.act);

  const delta = fixture.calls - baseline;
  assert(delta === 1, `expected exactly one coalesced computation across ten tabs, got ${delta}`);

  for (const root of roots) {
    await fixture.act(async () => root.unmount());
  }
  fixture.teardown();
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
