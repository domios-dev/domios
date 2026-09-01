// Run: npx tsx src/lib/pluginBubbleCache.test.ts
// codeterm#336 part C: gating, coalescing, and invalidation, against a fetcher this file controls call-by-call.

export {};

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

/** Polls a real condition to completion; fails deterministically past a
 * bounded safety deadline instead of guessing a fixed tick count. */
async function waitUntil(condition: () => boolean, maxTurns = 1000): Promise<void> {
  for (let i = 0; i < maxTurns; i++) {
    if (condition()) return;
    await Promise.resolve();
  }
  throw new Error(`waitUntil: condition not met within ${maxTurns} microtask turns`);
}

/** Drains pending microtasks — only for negative assertions where there is
 * no positive condition to poll for. */
async function drain(turns = 20): Promise<void> {
  for (let i = 0; i < turns; i++) await Promise.resolve();
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

type Bubble = import("./pluginBubbleCache").BubbleData;

async function setupCache() {
  const cache = await import("./pluginBubbleCache");
  cache.__resetPluginBubbleCacheForTest();

  let calls = 0;
  const pending: Array<Deferred<Bubble | null>> = [];
  cache.__setPluginBubbleFetcherForTest(async () => {
    calls++;
    const d = deferred<Bubble | null>();
    pending.push(d);
    return d.promise;
  });

  return {
    cache,
    get calls() {
      return calls;
    },
    resolveNext: (value: Bubble | null) => {
      const d = pending.shift();
      assert(d, "resolveNext called with no pending fetch");
      d!.resolve(value);
    },
    teardown: () => {
      cache.__resetPluginBubbleCacheForTest();
    },
  };
}

test("a hidden subscriber never fetches, even under an explicit invalidation", async () => {
  const fx = await setupCache();
  const sub = fx.cache.subscribePluginBubble("git", "/repo", "tab-1", false, () => {});

  assert(fx.calls === 0, `hidden subscribe must not fetch, got ${fx.calls}`);
  sub.setActive(false);
  await drain();
  assert(fx.calls === 0, `re-affirming hidden must not fetch, got ${fx.calls}`);

  fx.cache.invalidatePluginBubble("git", "/repo");
  await drain();
  assert(fx.calls === 0, `no invalidation reaches a hidden tab, got ${fx.calls}`);

  sub.unsubscribe();
  fx.teardown();
});

test("becoming visible fetches once and delivers the resolved value", async () => {
  const fx = await setupCache();
  const values: Array<Bubble | null> = [];
  const sub = fx.cache.subscribePluginBubble("git", "/repo", "tab-1", true, (v) => values.push(v));

  assert(fx.calls === 1, `activation must fetch exactly once, got ${fx.calls}`);
  fx.resolveNext({ label: "main" });
  await waitUntil(() => values.length > 0);
  assert(values.at(-1)?.label === "main", "onChange must deliver the resolved value");

  sub.unsubscribe();
  fx.teardown();
});

test("an explicit invalidation returns the new value, not a stale one", async () => {
  const fx = await setupCache();
  const values: Array<Bubble | null> = [];
  const sub = fx.cache.subscribePluginBubble("git", "/repo", "tab-1", true, (v) => values.push(v));
  fx.resolveNext({ label: "main" });
  await waitUntil(() => values.length >= 1);
  assert(values.at(-1)?.label === "main", "initial value is the pre-checkout branch");

  fx.cache.invalidatePluginBubble("git", "/repo");
  await waitUntil(() => fx.calls >= 2);
  assert(fx.calls === 2, `invalidation must trigger exactly one more fetch, got ${fx.calls}`);
  fx.resolveNext({ label: "feature" });
  await waitUntil(() => values.length >= 2);
  assert(values.at(-1)?.label === "feature", "invalidation must deliver the new branch, not the cached old one");

  sub.unsubscribe();
  fx.teardown();
});

test("two invalidations while a fetch is in flight coalesce to one trailing refresh", async () => {
  const fx = await setupCache();
  const sub = fx.cache.subscribePluginBubble("git", "/repo", "tab-1", true, () => {});
  assert(fx.calls === 1, "activation fetched once");

  fx.cache.invalidatePluginBubble("git", "/repo");
  fx.cache.invalidatePluginBubble("git", "/repo");
  assert(fx.calls === 1, "both invalidations arrived while the first fetch was still in flight");

  fx.resolveNext({ label: "main" });
  await waitUntil(() => fx.calls >= 2);
  assert(fx.calls === 2, `exactly one coalesced trailing refresh, got ${fx.calls}`);
  fx.resolveNext({ label: "feature" });
  await drain();
  assert(fx.calls === 2, "no third computation follows");

  sub.unsubscribe();
  fx.teardown();
});

test("no further computation without an event", async () => {
  const fx = await setupCache();
  const sub = fx.cache.subscribePluginBubble("git", "/repo", "tab-1", true, () => {});
  fx.resolveNext({ label: "main" });
  await waitUntil(() => fx.calls >= 1);
  await drain();
  assert(fx.calls === 1, `expected exactly one computation, got ${fx.calls}`);

  sub.unsubscribe();
  fx.teardown();
});

test("ten tabs sharing a repository coalesce to one in-flight computation", async () => {
  const fx = await setupCache();
  const subs = Array.from({ length: 10 }, (_, i) =>
    fx.cache.subscribePluginBubble("git", "/repo", `tab-${i}`, i === 0, () => {}),
  );
  assert(fx.calls === 1, `only the one active tab should have fetched, got ${fx.calls}`);

  fx.cache.invalidatePluginBubble("git", "/repo");
  assert(fx.calls === 1, "an invalidation while a fetch is already in flight must coalesce, not fetch again");

  fx.resolveNext({ label: "main" });
  await waitUntil(() => fx.calls >= 2);
  assert(fx.calls === 2, `the coalesced trailing refresh must fire exactly once more, got ${fx.calls}`);

  fx.resolveNext({ label: "main" });
  await drain();
  assert(fx.calls === 2, `no third computation should follow, got ${fx.calls}`);

  for (const sub of subs) sub.unsubscribe();
  fx.teardown();
});

test("a hidden->visible transition rechecks even a value another tab already fetched", async () => {
  const fx = await setupCache();
  const valuesA: Array<Bubble | null> = [];
  const subA = fx.cache.subscribePluginBubble("git", "/repo", "tab-a", true, (v) => valuesA.push(v));
  fx.resolveNext({ label: "main" });
  await waitUntil(() => valuesA.length >= 1);

  const valuesB: Array<Bubble | null> = [];
  const subB = fx.cache.subscribePluginBubble("git", "/repo", "tab-b", false, (v) => valuesB.push(v));
  assert(valuesB.at(-1)?.label === "main", "a hidden subscriber still receives the already-cached value");
  assert(fx.calls === 1, "subscribing hidden must not fetch");

  subB.setActive(true);
  await waitUntil(() => fx.calls >= 2);
  assert(fx.calls === 2, `tab B becoming visible must recheck even though a value was cached, got ${fx.calls}`);
  fx.resolveNext({ label: "feature" });
  await waitUntil(() => valuesB.at(-1)?.label === "feature");

  subA.unsubscribe();
  subB.unsubscribe();
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
