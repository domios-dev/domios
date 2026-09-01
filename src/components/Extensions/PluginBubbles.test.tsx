// Run: node scripts/run-ts-unit-tests.mjs PluginBubbles
// codeterm#336 part C: watch lifecycle and invalidation (cache: pluginBubbleCache.test.ts).

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

type WatchEvent = import("../../lib/repoWatch").RepoWatchEvent;
type WatchMode = "auto" | "manual" | "reject" | "unavailable";

/** Records every subscribe and unsubscribe; `active` is the net refcount the
 * daemon would still be holding. */
function makeWatchClient(mode: WatchMode, fixedIdentity?: string) {
  let handler: ((event: WatchEvent) => void) | null = null;
  let seq = 0;
  const pending: Array<() => void> = [];
  const state = {
    subscribeCalls: 0,
    unsubscribeCalls: 0,
    listenCalls: 0,
    listenFailures: 0,
    liveListeners: 0,
    active: new Set<string>(),
  };
  const identityOf = (cwd: string) => fixedIdentity ?? `id:${cwd}`;

  const client = {
    subscribe(cwd: string) {
      state.subscribeCalls++;
      if (mode === "reject") return Promise.reject(new Error("transport refused"));
      if (mode === "unavailable") {
        return Promise.resolve({ status: "unavailable", reason: "not_a_repository" });
      }
      const token = `tok-${++seq}`;
      const result = { status: "watching", token, identity: identityOf(cwd) };
      if (mode === "manual") {
        return new Promise<typeof result>((resolve) => {
          pending.push(() => {
            state.active.add(token);
            resolve(result);
          });
        });
      }
      state.active.add(token);
      return Promise.resolve(result);
    },
    unsubscribe(token: string) {
      state.unsubscribeCalls++;
      state.active.delete(token);
      return Promise.resolve(undefined);
    },
    listen(fn: (event: WatchEvent) => void) {
      state.listenCalls++;
      if (state.listenFailures > 0) {
        state.listenFailures--;
        return Promise.reject(new Error("event stream unavailable"));
      }
      handler = fn;
      state.liveListeners++;
      return Promise.resolve(() => {
        handler = null;
        state.liveListeners--;
      });
    },
  };

  return {
    client,
    state,
    identityOf,
    emit: (event: WatchEvent) => handler?.(event),
    flush: () => {
      while (pending.length > 0) pending.shift()?.();
    },
  };
}

async function setupFixture(
  pluginId = "fake-git",
  watchMode: WatchMode = "auto",
  fixedIdentity?: string,
) {
  const { act, createRoot, React, window } = await setupHappyDom();
  const bridge = await import("../../lib/bridge");
  const snapshotStore = await import("../../lib/pluginSnapshotStore");
  const cache = await import("../../lib/pluginBubbleCache");
  const repoWatch = await import("../../lib/repoWatch");
  const { PluginBubbles } = await import("./PluginBubbles");

  snapshotStore.__resetPluginSnapshotStoreForTest();
  cache.__resetPluginBubbleCacheForTest();
  repoWatch.__resetRepoWatchForTest();

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

  const watch = makeWatchClient(watchMode, fixedIdentity);
  repoWatch.__setRepoWatchClientForTest(watch.client);

  const host = window.document.createElement("div");
  window.document.body.appendChild(host);
  const root = createRoot(host as unknown as Element);

  return {
    act,
    React,
    root,
    host,
    cache,
    repoWatch,
    watch,
    PluginBubbles,
    pluginId,
    get calls() {
      return calls;
    },
    teardown: () => {
      bridge.__setInvokeForTest(null);
      cache.__resetPluginBubbleCacheForTest();
      snapshotStore.__resetPluginSnapshotStoreForTest();
      repoWatch.__resetRepoWatchForTest();
    },
  };
}

type Fixture = Awaited<ReturnType<typeof setupFixture>>;

function render(fx: Fixture, props: { cwd: string | null; tabId: string; isActive: boolean }) {
  return fx.act(async () => {
    fx.root.render(fx.React.createElement(fx.PluginBubbles, props));
  });
}

/** Ten visible bubbles on one repository, one mounted tree. */
function renderMany(fx: Fixture, cwd: string, count: number) {
  return fx.act(async () => {
    fx.root.render(
      fx.React.createElement(
        fx.React.Fragment,
        null,
        ...Array.from({ length: count }, (_, i) =>
          fx.React.createElement(fx.PluginBubbles, {
            key: `tab-${i}`,
            cwd,
            tabId: `tab-${i}`,
            isActive: true,
          }),
        ),
      ),
    );
  });
}

test("a hidden tab performs zero status-bubble provider calls", async () => {
  const fx = await setupFixture();
  await render(fx, { cwd: "/repo", tabId: "tab-hidden", isActive: false });

  // Drains the plugin-snapshot fetch (unrelated to bubble computation); a
  // hidden tab must still never call the bubble fetcher.
  await drain(fx.act);
  assert(fx.calls === 0, `hidden tab must never compute, got ${fx.calls} call(s)`);
  assert(
    fx.watch.state.subscribeCalls === 0,
    `hidden tab must not lease a watch, got ${fx.watch.state.subscribeCalls}`,
  );

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("a visible tab computes exactly once", async () => {
  const fx = await setupFixture();
  await render(fx, { cwd: "/repo", tabId: "tab-visible", isActive: true });

  await waitUntil(() => fx.calls >= 1, fx.act);
  assert(fx.calls === 1, `expected exactly one computation, got ${fx.calls}`);

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("becoming visible subscribes exactly once", async () => {
  const fx = await setupFixture();
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: false });
  await drain(fx.act);
  assert(fx.watch.state.subscribeCalls === 0, "hidden must not subscribe");

  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.watch.state.active.size >= 1, fx.act);
  assert(
    fx.watch.state.subscribeCalls === 1,
    `becoming visible subscribes once, got ${fx.watch.state.subscribeCalls}`,
  );

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("hiding unsubscribes exactly once", async () => {
  const fx = await setupFixture();
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.watch.state.active.size >= 1, fx.act);

  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: false });
  await waitUntil(() => fx.watch.state.unsubscribeCalls >= 1, fx.act);
  await drain(fx.act);
  assert(
    fx.watch.state.unsubscribeCalls === 1,
    `hiding unsubscribes once, got ${fx.watch.state.unsubscribeCalls}`,
  );
  assert(fx.watch.state.active.size === 0, "hiding leaves no refcount");

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("a visible-hidden-visible cycle leaves a net refcount of exactly one", async () => {
  const fx = await setupFixture();
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.watch.state.active.size >= 1, fx.act);
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: false });
  await waitUntil(() => fx.watch.state.active.size === 0, fx.act);
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.watch.state.active.size >= 1, fx.act);
  await drain(fx.act);

  assert(fx.watch.state.subscribeCalls === 2, `two subscribes, got ${fx.watch.state.subscribeCalls}`);
  assert(
    fx.watch.state.unsubscribeCalls === 1,
    `one unsubscribe, got ${fx.watch.state.unsubscribeCalls}`,
  );
  assert(fx.watch.state.active.size === 1, `net refcount 1, got ${fx.watch.state.active.size}`);

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("unmounting during an in-flight subscribe leaves no refcount", async () => {
  const fx = await setupFixture("fake-git", "manual");
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await drain(fx.act);
  assert(fx.watch.state.subscribeCalls === 1, "the subscribe was issued");
  assert(fx.watch.state.active.size === 0, "and has not resolved yet");

  await fx.act(async () => fx.root.unmount());
  await fx.act(async () => {
    fx.watch.flush();
  });
  await drain(fx.act);

  assert(
    fx.watch.state.unsubscribeCalls === 1,
    `the late-resolving lease is handed back, got ${fx.watch.state.unsubscribeCalls}`,
  );
  assert(fx.watch.state.active.size === 0, "unmount during an in-flight subscribe leaks nothing");
  fx.teardown();
});

test("a subscribe that rejects leaves no refcount and surfaces the unavailable state", async () => {
  const fx = await setupFixture("fake-git", "reject");
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.calls >= 1, fx.act);
  await drain(fx.act);

  assert(fx.watch.state.active.size === 0, "a rejected subscribe holds no reference");
  assert(
    fx.watch.state.unsubscribeCalls === 0,
    `nothing to release, got ${fx.watch.state.unsubscribeCalls} unsubscribe(s)`,
  );
  const pill = fx.host.querySelector(`[data-testid="plugin-bubble-${fx.pluginId}"]`);
  assert(pill, "the bubble still renders");
  assert(
    pill?.getAttribute("data-watch-state") === "unavailable",
    `an unwatched bubble is never rendered as live, got ${pill?.getAttribute("data-watch-state")}`,
  );

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("a named-unavailable subscribe result surfaces the same honest unknown", async () => {
  const fx = await setupFixture("fake-git", "unavailable");
  await render(fx, { cwd: "/not-a-repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.calls >= 1, fx.act);
  await drain(fx.act);

  assert(fx.watch.state.active.size === 0, "an unavailable repository holds no reference");
  const pill = fx.host.querySelector(`[data-testid="plugin-bubble-${fx.pluginId}"]`);
  assert(
    pill?.getAttribute("data-watch-state") === "unavailable",
    "a non-repository renders as unwatched, not as live",
  );
  assert(
    (pill?.getAttribute("title") ?? "").includes("not_a_repository"),
    `the discriminant is named in the tooltip, got ${pill?.getAttribute("title")}`,
  );

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("a repository-changed event for this identity recomputes exactly once", async () => {
  const fx = await setupFixture();
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.watch.state.active.size >= 1, fx.act);
  await drain(fx.act);
  const before = fx.calls;

  await fx.act(async () => {
    fx.watch.emit({ kind: "changed", identity: fx.watch.identityOf("/repo") });
  });
  await waitUntil(() => fx.calls > before, fx.act);
  await drain(fx.act);
  assert(fx.calls === before + 1, `one event, one recompute, got ${fx.calls - before}`);

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("an explicit Git action recomputes exactly once", async () => {
  const fx = await setupFixture();
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.watch.state.active.size >= 1, fx.act);
  await drain(fx.act);
  const before = fx.calls;

  await fx.act(async () => {
    fx.cache.invalidatePluginBubble(fx.pluginId, "/repo");
  });
  await waitUntil(() => fx.calls > before, fx.act);
  await drain(fx.act);
  assert(fx.calls === before + 1, `one action, one recompute, got ${fx.calls - before}`);

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("an event for a different identity does not invalidate", async () => {
  const fx = await setupFixture();
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.watch.state.active.size >= 1, fx.act);
  await drain(fx.act);
  const before = fx.calls;

  await fx.act(async () => {
    fx.watch.emit({ kind: "changed", identity: fx.watch.identityOf("/some/other/repo") });
  });
  await drain(fx.act);
  assert(fx.calls === before, `a foreign identity must not recompute, got ${fx.calls - before}`);

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("advancing virtual time arbitrarily far causes zero recomputes", async () => {
  const fx = await setupFixture();
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.watch.state.active.size >= 1, fx.act);
  await drain(fx.act);
  const before = fx.calls;

  const scheduled: Array<() => void> = [];
  let intervals = 0;
  const slot = globalThis as unknown as { setTimeout: unknown; setInterval: unknown };
  const realTimeout = slot.setTimeout as (fn: () => void, delay?: number) => unknown;
  const realInterval = slot.setInterval as (fn: () => void, delay?: number) => unknown;
  slot.setTimeout = (fn: () => void, delay?: number): unknown => {
    if ((delay ?? 0) >= 1) {
      scheduled.push(fn);
      return 0;
    }
    return realTimeout(fn, delay);
  };
  slot.setInterval = (fn: () => void, delay?: number): unknown => {
    intervals++;
    return realInterval(fn, delay);
  };

  try {
    await drain(fx.act);
    // Fire everything time could ever fire — arbitrarily far ahead.
    await fx.act(async () => {
      for (const fn of scheduled.splice(0)) fn();
    });
    await drain(fx.act);
  } finally {
    slot.setTimeout = realTimeout;
    slot.setInterval = realInterval;
  }

  assert(intervals === 0, `no polling interval may be scheduled, got ${intervals}`);
  assert(fx.calls === before, `time alone must not recompute, got ${fx.calls - before}`);

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("a tab-status change between idle and working causes zero recomputes", async () => {
  const fx = await setupFixture();
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.watch.state.active.size >= 1, fx.act);
  await drain(fx.act);
  const before = fx.calls;

  // Tab status is not a change signal; the bubble path subscribes to none of
  // it, and delivering it must recompute nothing.
  for (const status of ["idle", "working", "idle"]) {
    await fx.act(async () => {
      window.dispatchEvent(new CustomEvent("tab-status-changed", { detail: { tabId: "tab-1", status } }));
    });
  }
  await drain(fx.act);
  assert(fx.calls === before, `tab status must not recompute, got ${fx.calls - before}`);

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("ten visible bubbles on one identity plus one event produce exactly one computation", async () => {
  const fx = await setupFixture();
  await renderMany(fx, "/repo", 10);
  await waitUntil(() => fx.watch.state.active.size >= 10, fx.act);
  await drain(fx.act);
  const before = fx.calls;

  await fx.act(async () => {
    fx.watch.emit({ kind: "changed", identity: fx.watch.identityOf("/repo") });
  });
  await waitUntil(() => fx.calls > before, fx.act);
  await drain(fx.act);
  assert(
    fx.calls === before + 1,
    `ten bubbles, one event, one statusBubble computation and one git status, got ${fx.calls - before}`,
  );

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

/** Two tabs whose cwds are aliases of one repository. */
function renderAliases(fx: Fixture, first: string, second: string) {
  return fx.act(async () => {
    fx.root.render(
      fx.React.createElement(
        fx.React.Fragment,
        null,
        fx.React.createElement(fx.PluginBubbles, { key: "a", cwd: first, tabId: "tab-a", isActive: true }),
        fx.React.createElement(fx.PluginBubbles, { key: "b", cwd: second, tabId: "tab-b", isActive: true }),
      ),
    );
  });
}

test("an unresolved subscribe never renders as live", async () => {
  const fx = await setupFixture("fake-git", "manual");
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.calls >= 1, fx.act);
  await drain(fx.act);

  assert(fx.watch.state.active.size === 0, "the subscribe has not resolved");
  const pill = fx.host.querySelector(`[data-testid="plugin-bubble-${fx.pluginId}"]`);
  assert(pill, "the bubble renders while the watch is still pending");
  assert(
    pill?.getAttribute("data-watch-state") === "pending",
    `an unproven watch must not read as live, got ${pill?.getAttribute("data-watch-state")}`,
  );

  await fx.act(async () => {
    fx.watch.flush();
  });
  await waitUntil(
    () =>
      fx.host
        .querySelector(`[data-testid="plugin-bubble-${fx.pluginId}"]`)
        ?.getAttribute("data-watch-state") === "live",
    fx.act,
  );

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("a hidden tab never renders as live either", async () => {
  const fx = await setupFixture();
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.watch.state.active.size >= 1, fx.act);
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: false });
  await drain(fx.act);

  const pill = fx.host.querySelector(`[data-testid="plugin-bubble-${fx.pluginId}"]`);
  assert(
    pill?.getAttribute("data-watch-state") === "idle",
    `a tab holding no watch must not read as live, got ${pill?.getAttribute("data-watch-state")}`,
  );

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("an event-stream failure surfaces and never leaves a lease claiming live", async () => {
  const fx = await setupFixture();
  fx.watch.state.listenFailures = 1;
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(() => fx.watch.state.active.size >= 1, fx.act);
  await drain(fx.act);

  const pill = fx.host.querySelector(`[data-testid="plugin-bubble-${fx.pluginId}"]`);
  assert(
    pill?.getAttribute("data-watch-state") === "unavailable",
    `a failed stream must not read as live, got ${pill?.getAttribute("data-watch-state")}`,
  );
  assert(
    (pill?.getAttribute("title") ?? "").includes("stream_unavailable"),
    `the discriminant is named, got ${pill?.getAttribute("title")}`,
  );

  // Hiding releases everything, and a fresh lifecycle opens a new stream.
  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: false });
  await drain(fx.act);
  let debug = fx.repoWatch.__repoWatchDebugForTest();
  assert(debug.leases === 0, `no lease retained, got ${debug.leases}`);
  assert(!debug.streamOpen, "no rejected stream retained");
  assert(fx.watch.state.liveListeners === 0, `no listener retained, got ${fx.watch.state.liveListeners}`);

  await render(fx, { cwd: "/repo", tabId: "tab-1", isActive: true });
  await waitUntil(
    () =>
      fx.host
        .querySelector(`[data-testid="plugin-bubble-${fx.pluginId}"]`)
        ?.getAttribute("data-watch-state") === "live",
    fx.act,
  );
  assert(fx.watch.state.listenCalls === 2, `a fresh stream was opened, got ${fx.watch.state.listenCalls}`);
  debug = fx.repoWatch.__repoWatchDebugForTest();
  assert(debug.streamOpen, "the new stream is retained");

  await fx.act(async () => fx.root.unmount());
  fx.teardown();
});

test("a first-listener rejection retains no listener", async () => {
  const { openRepoWatchStream } = await import("../../lib/repoWatch");
  let registrations = 0;
  const subscribe = (async () => {
    registrations++;
    throw new Error("first registration failed");
  }) as unknown as Parameters<typeof openRepoWatchStream>[0];

  let rejected = false;
  await openRepoWatchStream(subscribe, () => {}).catch(() => {
    rejected = true;
  });
  assert(rejected, "the stream must reject, not resolve half-open");
  assert(registrations === 1, `one attempt and nothing to unlisten, got ${registrations}`);
});

test("a second-listener rejection unlistens the first", async () => {
  const { openRepoWatchStream } = await import("../../lib/repoWatch");
  let registrations = 0;
  let unlistens = 0;
  const subscribe = (async () => {
    registrations++;
    if (registrations === 2) throw new Error("second registration failed");
    return () => {
      unlistens++;
    };
  }) as unknown as Parameters<typeof openRepoWatchStream>[0];

  let rejected = false;
  await openRepoWatchStream(subscribe, () => {}).catch(() => {
    rejected = true;
  });
  assert(rejected, "the stream must reject");
  assert(registrations === 2, `both registrations attempted, got ${registrations}`);
  assert(unlistens === 1, `the first listener must be released, got ${unlistens}`);
});

test("two cwd aliases of one repository share a cache entry and one computation", async () => {
  const fx = await setupFixture("fake-git", "auto", "id:/repo");
  await renderAliases(fx, "/repo", "/repo/../repo");
  await waitUntil(() => fx.watch.state.active.size >= 2, fx.act);
  await drain(fx.act);

  assert(fx.watch.state.subscribeCalls === 2, "each visible tab leases its own watch");
  const before = fx.calls;

  await fx.act(async () => {
    fx.watch.emit({ kind: "changed", identity: "id:/repo" });
  });
  await waitUntil(() => fx.calls > before, fx.act);
  await drain(fx.act);
  assert(
    fx.calls === before + 1,
    `two aliases are one identity: one event, one git status, got ${fx.calls - before}`,
  );

  await fx.act(async () => fx.root.unmount());
  await drain(fx.act);
  assert(
    fx.watch.state.unsubscribeCalls === 2 && fx.watch.state.active.size === 0,
    `both alias leases release, got ${fx.watch.state.unsubscribeCalls}`,
  );
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
