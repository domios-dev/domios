import test from "node:test";
import assert from "node:assert/strict";
import { snapshotAfterEvent } from "./snapshots.mjs";

test("a successful snapshot does not wait for another event", async () => {
  const events = { next: () => assert.fail("Unexpected event wait") };
  assert.deepEqual(await snapshotAfterEvent(events, async () => ({ ready: true })), { ready: true });
});

test("a refused snapshot waits for an explicit event before rereading", async () => {
  let reads = 0, release;
  const witness = new Promise((resolve) => { release = resolve; });
  const events = { next: () => { assert.equal(reads, 1); release(); return witness; } };
  const result = await snapshotAfterEvent(events, async () => {
    if (++reads === 1) throw Object.assign(new Error("Unavailable snapshot"), { status: 503 });
    return { ready: true };
  });
  assert.deepEqual(result, { ready: true });
  assert.equal(reads, 2);
});

test("other failures are propagated without retrying the read", async () => {
  const failure = Object.assign(new Error("Access refused"), { status: 403 });
  let reads = 0;
  const events = { next: () => assert.fail("Unexpected retry") };
  await assert.rejects(snapshotAfterEvent(events, async () => { reads++; throw failure; }), failure);
  assert.equal(reads, 1);
});

test("the event deadline ends a snapshot retry without another read", async () => {
  const deadline = new Error("Observation deadline exceeded");
  let reads = 0;
  const events = { next: async () => { throw deadline; } };
  await assert.rejects(snapshotAfterEvent(events, async () => {
    reads++;
    throw Object.assign(new Error("Unavailable snapshot"), { status: 503 });
  }), deadline);
  assert.equal(reads, 1);
});
