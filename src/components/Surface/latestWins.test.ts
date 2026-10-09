// Run: npx tsx src/components/Surface/latestWins.test.ts

import assert from "node:assert/strict";
import { createLatestWins } from "./latestWins";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

async function flush() {
  for (let index = 0; index < 5; index += 1) await Promise.resolve();
}

async function main() {
  const started: number[] = [];
  const gates = new Map<number, ReturnType<typeof deferred>>();
  const queue = createLatestWins<number>((item) => {
    started.push(item);
    const gate = deferred();
    gates.set(item, gate);
    return gate.promise;
  });

  queue.push(1);
  queue.push(2);
  queue.push(3);
  queue.push(4);
  await flush();
  assert.deepEqual(started, [1], "only one item runs at a time");
  gates.get(1)!.resolve();
  await flush();
  assert.deepEqual(started, [1, 4], "intermediate items are dropped for the newest");
  gates.get(4)!.resolve();
  await flush();
  queue.push(5);
  await flush();
  assert.deepEqual(started, [1, 4, 5], "an idle queue runs the next item immediately");

  const errors: unknown[] = [];
  const failing = createLatestWins<number>(async (item) => { if (item === 1) throw new Error("decode"); started.push(item); }, (error) => errors.push(error));
  failing.push(1);
  await flush();
  failing.push(2);
  await flush();
  assert.equal(errors.length, 1, "a failing item reports its error");
  assert.equal(started[started.length - 1], 2, "a failure does not stall the queue");

  const disposedRuns: number[] = [];
  const disposable = createLatestWins<number>(async (item) => { disposedRuns.push(item); });
  disposable.dispose();
  disposable.push(1);
  await flush();
  assert.deepEqual(disposedRuns, [], "a disposed queue ignores new items");

  console.log("✓ latestWins tests green");
}

void main().catch((error) => { console.error(error); process.exit(1); });
