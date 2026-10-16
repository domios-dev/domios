import assert from "node:assert/strict";
import test from "node:test";
import { createChatAttachFlight } from "./chatAttachFlight";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

test("same attachment requests share one in-flight operation", async () => {
  const flight = createChatAttachFlight<number>();
  const response = deferred<number>();
  let calls = 0;
  const start = () => { calls++; return response.promise; };
  const first = flight.run("same-lease", start);
  const second = flight.run("same-lease", start);
  assert.equal(calls, 1);
  response.resolve(7);
  assert.deepEqual(await first, { kind: "completed", value: 7 });
  assert.deepEqual(await second, { kind: "completed", value: 7 });
});

test("authority changes serialize and only the latest pending attachment starts", async () => {
  const flight = createChatAttachFlight<number>();
  const firstReply = deferred<number>();
  const latestStarted = deferred<void>();
  const calls: string[] = [];
  const first = flight.run("first", () => { calls.push("first"); return firstReply.promise; });
  const skipped = flight.run("skipped", async () => { calls.push("skipped"); return 2; });
  const latest = flight.run("latest", async () => { calls.push("latest"); latestStarted.resolve(); return 3; });
  assert.deepEqual(calls, ["first"]);
  assert.deepEqual(await skipped, { kind: "superseded" });
  firstReply.resolve(1);
  await first;
  await latestStarted.promise;
  assert.deepEqual(await latest, { kind: "completed", value: 3 });
  assert.deepEqual(calls, ["first", "latest"]);
});

test("a failed operation releases the next authority without a timer", async () => {
  const flight = createChatAttachFlight<number>();
  const firstReply = deferred<number>();
  const first = flight.run("first", () => firstReply.promise);
  const failed = assert.rejects(first, /opaque-failure/);
  const next = flight.run("next", async () => 9);
  firstReply.reject(new Error("opaque-failure"));
  await failed;
  assert.deepEqual(await next, { kind: "completed", value: 9 });
});

test("unmount cancels a pending operation without starting its request", async () => {
  const flight = createChatAttachFlight<number>();
  const firstReply = deferred<number>();
  const first = flight.run("first", () => firstReply.promise);
  let pendingCalls = 0;
  const pending = flight.run("pending", async () => { pendingCalls++; return 2; });
  flight.cancelPending();
  assert.deepEqual(await pending, { kind: "superseded" });
  firstReply.resolve(1);
  await first;
  assert.equal(pendingCalls, 0);
});
