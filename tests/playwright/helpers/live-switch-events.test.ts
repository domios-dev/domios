import assert from "node:assert/strict";
import { test } from "node:test";
import type { ApiClient } from "./api-client";
import { TabEvents } from "../suites/16-chat-view/chat-send-matrix.live.helpers";

function streamFixture(status: string) {
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  const body = new ReadableStream<Uint8Array>({ start(value) { controller = value; } });
  const requests: string[] = [];
  const api = {
    openEventStream: async () => new Response(body),
    get: async (url: string) => {
      requests.push(url);
      return url.endsWith("/status") ? { status } : [];
    },
  } as unknown as ApiClient;
  return { api, requests, emit: (frame: unknown) => controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify(frame)}\n\n`)) };
}

test("lagged stream resync reads only own state and resumes subsequent frames", async () => {
  const fixture = streamFixture("working");
  const events = await TabEvents.open(fixture.api, "owned");
  try {
    const working = events.waitFor(event => event.seq > 3 && event.data.status === "Working", 1000, "resynced working");
    fixture.emit({ event_type: "overflow", data: { reason: "lagged", boot: "new-boot", next_seq: 10 } });
    const state = await working;
    assert.equal(state.seq, 9);
    assert.equal(state.data.resynced, true);
    assert.equal(events.latestSequence(), 9);
    assert.deepEqual(fixture.requests, ["/tabs/owned/status", "/tabs/owned/chat-send"]);
    assert.deepEqual(events.resyncs, [{ boot: "new-boot", nextSeq: 10, status: "Working" }]);
    const idle = events.waitFor(event => event.seq > 9 && event.data.status === "Idle", 1000, "later idle");
    fixture.emit({ seq: 10, event_type: "tab-status-changed", data: { tab_id: "owned", status: "Idle" } });
    assert.equal((await idle).data.resynced, undefined);
  } finally { await events.close(); }
});

test("resync cannot manufacture a missed Working transition from current Idle", async () => {
  const fixture = streamFixture("idle");
  const events = await TabEvents.open(fixture.api, "owned");
  try {
    const idle = events.waitFor(event => event.data.status === "Idle", 1000, "current idle");
    fixture.emit({ event_type: "overflow", data: { reason: "lagged", boot: "boot", next_seq: 4 } });
    await idle;
    const working = events.waitFor(event => event.data.status === "Working", 1000, "actual future working");
    fixture.emit({ seq: 4, event_type: "tab-status-changed", data: { tab_id: "owned", status: "Working" } });
    assert.equal((await working).seq, 4);
  } finally { await events.close(); }
});

test("unexpected DEV boot change remains a failure", async () => {
  const fixture = streamFixture("Idle");
  const events = await TabEvents.open(fixture.api, "owned");
  try {
    const result = events.waitFor(() => true, 1000, "event");
    fixture.emit({ event_type: "overflow", data: { reason: "boot-changed", boot: "other", next_seq: 4 } });
    await assert.rejects(result, /tab event stream overflow/);
    assert.deepEqual(fixture.requests, []);
  } finally { await events.close(); }
});

test("record observers receive own stream wakeups and detach after completion", async () => {
  const fixture = streamFixture("idle");
  const events = await TabEvents.open(fixture.api, "owned");
  const received: number[] = [];
  const detach = events.subscribe(event => { received.push(event.seq); });
  try {
    const first = events.waitFor(event => event.seq === 1, 1000, "first wake");
    fixture.emit({ seq: 1, event_type: "tab-status-changed", data: { tab_id: "owned", status: "Idle" } });
    await first;
    detach();
    const second = events.waitFor(event => event.seq === 2, 1000, "detached wake");
    fixture.emit({ seq: 2, event_type: "tab-status-changed", data: { tab_id: "owned", status: "Working" } });
    await second;
    assert.deepEqual(received, [1]);
  } finally { await events.close(); }
});
