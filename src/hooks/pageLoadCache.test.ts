import assert from "node:assert/strict";
import { createPageLoadCache } from "./pageLoadCache";

async function main() {
  let calls = 0;
  let finish!: (value: number) => void;
  const cache = createPageLoadCache((key: string) => {
    calls++;
    return key === "held" ? new Promise<number>((resolve) => { finish = resolve; }) : Promise.resolve(calls);
  });
  const first = cache.read("held");
  assert.equal(cache.read("held"), first);
  assert.equal(calls, 1);
  finish(7);
  assert.equal(await first, 7);
  assert.equal(await cache.read("held"), 7);
  await cache.read("other");
  await cache.refresh((key) => key === "other");
  assert.equal(calls, 3);
  assert.equal(await cache.read("held"), 7);
  assert.equal(await cache.read("other"), 3);
  console.log("✓ page cache shares pending and resolved loads and refreshes matching scopes only");

  let failures = 0;
  const failing = createPageLoadCache(async (_key: string) => {
    failures++;
    if (failures === 1) throw new Error("unavailable");
    return 9;
  });
  await assert.rejects(failing.read("scope"));
  assert.equal(await failing.read("scope"), 9);
  assert.equal(failures, 2);
  cache.clear();
  assert.equal(await cache.read("other"), 4);
  console.log("✓ failed reads can recover and page reset discards cached values");
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
