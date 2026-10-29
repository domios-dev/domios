import assert from "node:assert/strict";
import { test } from "node:test";
import { createRefreshCoalescer } from "./refreshCoalescer";
import { mountSnapshotReadiness } from "./snapshotReadiness";

function fixture(initialVersion = 0) {
  let version = initialVersion;
  const listeners = new Set<() => void>();
  const reads: Promise<boolean>[] = [];
  let readable = false;
  let workspace: string | null = null;
  const interrupted = new DOMException("Initial state read interrupted", "AbortError");
  const refresh = createRefreshCoalescer(async () => {
    if (!readable) throw interrupted;
    workspace = "restored-workspace";
    return true;
  }, error => error === interrupted);
  const mount = () => mountSnapshotReadiness({
    readyVersion: () => version,
    subscribeReady(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    refresh() { reads.push(refresh.request()); },
  });
  return {
    mount, reads,
    workspace: () => workspace,
    releaseSnapshot() { readable = true; },
    acknowledgeStream() { version++; for (const listener of listeners) listener(); },
    repeatAcknowledgement() { for (const listener of listeners) listener(); },
    listeners: () => listeners.size,
  };
}

test("first launch restores after an interrupted read whose state event preceded stream attachment", async () => {
  const f = fixture();
  const stop = f.mount();
  assert.equal(await f.reads[0], false);
  assert.equal(f.workspace(), null);
  f.releaseSnapshot();
  assert.equal(f.reads.length, 1);
  f.acknowledgeStream();
  assert.equal(await f.reads[1], true);
  assert.equal(f.workspace(), "restored-workspace");
  f.repeatAcknowledgement();
  assert.equal(f.reads.length, 2);
  stop();
});

test("a late subscriber queries restored state when stream readiness already happened", async () => {
  const f = fixture(1);
  f.releaseSnapshot();
  const stop = f.mount();
  assert.equal(await f.reads[0], true);
  assert.equal(f.workspace(), "restored-workspace");
  assert.equal(f.reads.length, 1);
  stop();
});

test("each new stream attachment reconciles once and disposed listeners stay retired", async () => {
  const f = fixture(1);
  f.releaseSnapshot();
  const stop = f.mount();
  await f.reads[0];
  f.acknowledgeStream();
  assert.equal(await f.reads[1], true);
  stop();
  assert.equal(f.listeners(), 0);
  f.acknowledgeStream();
  assert.equal(f.reads.length, 2);
  const stopReplacement = f.mount();
  assert.equal(await f.reads[2], true);
  stopReplacement();
});

test("readiness during a refused read queues a fresh query without concurrent snapshots", async () => {
  let acknowledge: () => void = () => {};
  let version = 0;
  let refuse: (error: Error) => void = () => {};
  let calls = 0;
  const busy = new Error("snapshot busy");
  const reads: Promise<boolean>[] = [];
  const first = new Promise<boolean>((_resolve, reject) => { refuse = reject; });
  const refresh = createRefreshCoalescer(() => ++calls === 1 ? first : Promise.resolve(true), error => error === busy);
  const stop = mountSnapshotReadiness({
    readyVersion: () => version,
    subscribeReady(listener) { acknowledge = listener; return () => {}; },
    refresh() { reads.push(refresh.request()); },
  });
  version++;
  acknowledge();
  assert.equal(calls, 1);
  refuse(busy);
  assert.deepEqual(await Promise.all(reads), [false, true]);
  assert.equal(calls, 2);
  stop();
});
