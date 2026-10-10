import test from "node:test";
import assert from "node:assert/strict";
import { snapshotAfterEvent, tabSnapshot } from "./snapshots.mjs";

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

test("before-restart tab snapshot opens SSE before a busy mux read and retains full records", async () => {
  let opened = false, closed = false, reads = 0, waits = 0;
  const events = {
    open: async () => { opened = true; },
    next: async () => { assert.equal(reads, 1); waits++; return { event_type: "mux-state-changed" }; },
    close: () => { closed = true; },
  };
  const before = await tabSnapshot({ createEvents: () => events,
    summaries: async () => [{ id: "owned", boot_stage: "summary" }],
    state: async () => {
      assert.equal(opened, true);
      if (++reads === 1) throw Object.assign(new Error("busy"), { status: 503 });
      return { tabs: { owned: { id: "owned", boot_stage: "agent_ready", agent_session_id: "session" } } };
    },
  });
  assert.deepEqual(before, [{ id: "owned", boot_stage: "agent_ready", agent_session_id: "session" }]);
  assert.equal(waits, 1);
  assert.equal(closed, true);
});

test("probe snapshots reuse their bounded caller stream without closing it", async () => {
  const events = { open: () => assert.fail("already open"), close: () => assert.fail("caller owns stream"), next: () => assert.fail("ready") };
  assert.deepEqual(await tabSnapshot({ events, summaries: async () => [], state: async () => ({ tabs: {} }) }), []);
});

test("a before-restart snapshot deadline closes the owned stream without another read", async () => {
  let reads = 0, closed = false;
  const deadline = new Error("deadline");
  const events = { open: async () => {}, next: async () => { throw deadline; }, close: () => { closed = true; } };
  await assert.rejects(tabSnapshot({ createEvents: () => events,
    summaries: async () => [], state: async () => { reads++; throw Object.assign(new Error("busy"), { status: 503 }); },
  }), deadline);
  assert.equal(reads, 1);
  assert.equal(closed, true);
});

test("busy tab snapshots ignore unrelated SSE events before reading again", async () => {
  const queue = [{ event_type: "output" }, { event_type: "mux-state-changed" }];
  let reads = 0;
  const events = { next: async () => { assert.equal(reads, 1); return queue.shift(); } };
  await tabSnapshot({ events, summaries: async () => [], state: async () => {
    if (++reads === 1) throw Object.assign(new Error("busy"), { status: 503 });
    assert.equal(queue.length, 0);
    return { tabs: {} };
  } });
  assert.equal(reads, 2);
});
