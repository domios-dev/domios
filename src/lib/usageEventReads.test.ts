import assert from "node:assert/strict";
import test from "node:test";
import { readUsageOnEvent } from "./usageEventReads";

test("one broadcast shares reads across consumers without merging a later update", async () => {
  const pending: Array<(value: number) => void> = [];
  const read = () => new Promise<number>(resolve => pending.push(resolve));
  const event = {};
  const args = { provider: "fixture", accountId: undefined };
  const first = readUsageOnEvent(event, "usage_latest", args, read);
  const consumers = Array.from({ length: 20 }, () => readUsageOnEvent(event, "usage_latest", { ...args }, read));
  assert.ok(consumers.every(result => result === first));
  assert.equal(pending.length, 1);
  const later = readUsageOnEvent({}, "usage_latest", args, read);
  assert.equal(pending.length, 2);
  pending[1](2);
  assert.equal(await later, 2);
  pending[0](1);
  assert.deepEqual(await Promise.all(consumers), Array(20).fill(1));
});

test("commands, accounts, ranges and explicit reads stay independent", async () => {
  let calls = 0;
  const read = async () => ++calls;
  const event = {};
  for (const [command, args] of [
    ["usage_latest", { provider: "a" }],
    ["usage_latest", { provider: "b" }],
    ["usage_latest", { provider: "a", accountId: "own" }],
    ["usage_history", { provider: "a", since: 1 }],
    ["usage_history", { provider: "a", since: 2 }],
    ["usage_projection", { provider: "a" }],
  ] as const) await readUsageOnEvent(event, command, args, read);
  await readUsageOnEvent(undefined, "usage_latest", { provider: "a" }, read);
  await readUsageOnEvent(undefined, "usage_latest", { provider: "a" }, read);
  assert.equal(calls, 8);
});

test("failed broadcast reads do not prevent a later event or explicit retry", async () => {
  const event = {};
  const failure = new Error("offline");
  const first = readUsageOnEvent(event, "usage_latest", {}, async () => { throw failure; });
  const second = readUsageOnEvent(event, "usage_latest", {}, async () => 2);
  assert.equal(first, second);
  await assert.rejects(first, error => error === failure);
  assert.equal(await readUsageOnEvent({}, "usage_latest", {}, async () => 3), 3);
  assert.equal(await readUsageOnEvent(undefined, "usage_latest", {}, async () => 4), 4);
});
