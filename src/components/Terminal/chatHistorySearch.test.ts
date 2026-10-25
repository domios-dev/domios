import assert from "node:assert/strict";
import { test } from "node:test";
import { searchOlderChatHistory } from "./chatHistorySearch";

test("empty consumed pages continue to a non-empty page", async () => {
  const requests: number[] = [];
  const result = await searchOlderChatHistory(800_000, async cursor => {
    requests.push(cursor);
    return cursor === 800_000 ? { cursor: 403_000, consumedBytes: 397_000, rows: 0 }
      : { cursor: 0, consumedBytes: 403_000, rows: 2 };
  });
  assert.deepEqual(requests, [800_000, 403_000]);
  assert.equal(result.kind, "reveal");
});

test("cursor zero ends an empty search", async () => {
  let requests = 0;
  const result = await searchOlderChatHistory(100, async () => { requests++; return { cursor: 0, consumedBytes: 100, rows: 0 }; });
  assert.deepEqual(result, { kind: "stop", reason: "exhausted" });
  assert.equal(requests, 1);
});

for (const cursor of [100, 101, -1]) {
  test(`invalid cursor ${cursor} is an error without another request`, async () => {
    let requests = 0;
    const result = await searchOlderChatHistory(100, async () => { requests++; return { cursor, consumedBytes: 0, rows: 0 }; });
    assert.deepEqual(result, { kind: "error", reason: "stalled_cursor" });
    assert.equal(requests, 1);
  });
}

test("eight empty item pages stop and the next click resumes from the consumed cursor", async () => {
  const requests: number[] = [];
  const load = async (cursor: number) => { requests.push(cursor); return { cursor: cursor - 1, consumedBytes: 0, rows: 0 }; };
  assert.deepEqual(await searchOlderChatHistory(100, load), { kind: "stop", reason: "limit" });
  assert.deepEqual(requests, [100, 99, 98, 97, 96, 95, 94, 93]);
  assert.equal((await searchOlderChatHistory(92, async cursor => {
    requests.push(cursor); return { cursor: 91, consumedBytes: 0, rows: 1 };
  })).kind, "reveal");
  assert.equal(requests[requests.length - 1], 92);
});

test("the byte budget stops before eight pages and bounds the final request", async () => {
  const budgets: number[] = [];
  const result = await searchOlderChatHistory(1000, async (cursor, maxBytes) => {
    budgets.push(maxBytes);
    const consumedBytes = Math.min(397, maxBytes);
    return { cursor: cursor - consumedBytes, consumedBytes, rows: 0 };
  }, 700);
  assert.deepEqual(budgets, [700, 303]);
  assert.deepEqual(result, { kind: "stop", reason: "limit" });
});

test("rejected authority ends traversal", async () => {
  let requests = 0;
  assert.deepEqual(await searchOlderChatHistory(100, async () => { requests++; return null; }), { kind: "none" });
  assert.equal(requests, 1);
});

test("the default byte bound consumes at most two one-MiB windows", async () => {
  const budgets: number[] = [];
  const result = await searchOlderChatHistory(10_000_000, async (cursor, maxBytes) => {
    budgets.push(maxBytes);
    return { cursor: cursor - maxBytes, consumedBytes: maxBytes, rows: 0 };
  });
  assert.deepEqual(budgets, [1_048_576, 1_048_576]);
  assert.deepEqual(result, { kind: "stop", reason: "limit" });
});


test("raw rows do not stop a scan until the rendered conversation page is filled", async () => {
  const requests: number[] = [];
  const result = await searchOlderChatHistory(300, async cursor => {
    requests.push(cursor);
    return { cursor: cursor - 100, consumedBytes: 100, rows: 150 };
  }, 1000, page => page.cursor <= 100);
  assert.deepEqual(requests, [300, 200]);
  assert.equal(result.kind, "reveal");
});
