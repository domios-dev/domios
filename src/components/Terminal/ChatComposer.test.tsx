import assert from "node:assert/strict";
import test from "node:test";
import type { DelayedMessage } from "./delayedSend";
import { createDelayedMessageFeed } from "./delayedMessageFeed";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function fixture() {
  let changed!: (tabId?: string) => void;
  let stopped = 0;
  let calls = 0;
  const published: DelayedMessage[][] = [];
  const feed = createDelayedMessageFeed("one", {
    load: async () => { calls++; return [{ id: String(calls) }] as DelayedMessage[]; },
    publish: (messages) => published.push(messages),
    subscribe: async (callback) => { changed = callback; return () => { stopped++; }; },
  });
  return { feed, changed: (tabId?: string) => changed(tabId), published, calls: () => calls, stopped: () => stopped };
}

test("delayed messages refresh on initial binding and relevant events", async () => {
  const harness = fixture();
  await harness.feed.ready;
  assert.equal(harness.calls(), 1);
  harness.changed("other");
  assert.equal(harness.calls(), 1);
  harness.changed("one");
  await Promise.resolve();
  assert.equal(harness.published[harness.published.length - 1]?.[0].id, "2");
  harness.changed();
  await Promise.resolve();
  assert.equal(harness.calls(), 3);
  harness.feed.dispose();
  harness.changed("one");
  assert.equal(harness.calls(), 3);
  assert.equal(harness.stopped(), 1);
});

test("delayed messages discard obsolete responses", async () => {
  const loads = [deferred<DelayedMessage[]>(), deferred<DelayedMessage[]>()];
  const started = deferred<void>();
  let calls = 0;
  const published: DelayedMessage[][] = [];
  const feed = createDelayedMessageFeed("one", {
    load: () => {
      const result = loads[calls++].promise;
      started.resolve();
      return result;
    },
    publish: (messages) => published.push(messages),
    subscribe: async () => () => {},
  });
  await started.promise;
  const latest = feed.refresh();
  loads[1].resolve([{ id: "new" }] as DelayedMessage[]);
  await latest;
  loads[0].resolve([{ id: "old" }] as DelayedMessage[]);
  await feed.ready;
  assert.deepEqual(published.map((messages) => messages[0].id), ["new"]);
  feed.dispose();
});

test("late listener binding is released after disposal without fetching", async () => {
  const binding = deferred<() => void>();
  let stopped = 0;
  let calls = 0;
  const feed = createDelayedMessageFeed("one", {
    load: async () => { calls++; return []; },
    publish: () => assert.fail("disposed feed published"),
    subscribe: () => binding.promise,
  });
  feed.dispose();
  binding.resolve(() => { stopped++; });
  await feed.ready;
  assert.equal(stopped, 1);
  assert.equal(calls, 0);
});
