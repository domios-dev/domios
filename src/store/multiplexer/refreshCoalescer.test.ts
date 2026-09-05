// Run: npx tsx src/store/multiplexer/refreshCoalescer.test.ts
// The in-flight ceiling, asserted on fetch counts rather than on rendered
// output: a burst of frames must not become a burst of full-state fetches.

import { createRefreshCoalescer } from "./refreshCoalescer";

let failures = 0;
async function test(name: string, fn: () => Promise<void> | void) {
  try {
    await fn();
    console.log(`  [ok] ${name}`);
  } catch (e) {
    failures++;
    console.error(`  [x] ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

/** A fetch the test completes by hand, recording its own concurrency. */
function controllable() {
  let inFlight = 0;
  let maxInFlight = 0;
  let calls = 0;
  const releases: Array<() => void> = [];
  const run = (): Promise<boolean> => {
    calls++;
    inFlight++;
    if (inFlight > maxInFlight) maxInFlight = inFlight;
    return new Promise<boolean>((resolve) => {
      releases.push(() => {
        inFlight--;
        resolve(true);
      });
    });
  };
  return {
    run,
    calls: () => calls,
    maxInFlight: () => maxInFlight,
    /** Complete the oldest open fetch. */
    releaseNext() {
      const release = releases.shift();
      assert(release, "expected an open fetch to release");
      release();
    },
    open: () => releases.length,
  };
}

const tick = (): Promise<void> => new Promise((r) => queueMicrotask(() => r()));

void (async () => {
  console.log("refresh coalescer");

  await test("a burst of 200 requests costs two fetches, never two at once", async () => {
    const backend = controllable();
    const coalescer = createRefreshCoalescer(backend.run);
    const requests = Array.from({ length: 200 }, () => coalescer.request());
    await tick();
    // The first request cannot be answered by a fetch older than itself, so it
    // opens one immediately; the other 199 share the single follow-up.
    assert(backend.calls() === 1, `expected 1 fetch open, got ${backend.calls()}`);
    assert(coalescer.inFlight() === 1, "one fetch in flight");
    assert(coalescer.waiting() === 199, `expected 199 waiting, got ${coalescer.waiting()}`);
    backend.releaseNext();
    await tick();
    assert(backend.calls() === 2, `expected the follow-up to be the 2nd fetch, got ${backend.calls()}`);
    backend.releaseNext();
    await Promise.all(requests);
    assert(backend.calls() === 2, `200 requests cost ${backend.calls()} fetches, expected 2`);
    assert(backend.maxInFlight() === 1, `max in flight was ${backend.maxInFlight()}`);
  });

  await test("the ceiling holds across a sustained storm", async () => {
    const backend = controllable();
    const coalescer = createRefreshCoalescer(backend.run);
    const requests: Array<Promise<boolean>> = [];
    // 20 rounds of 50 frames, each round landing while a fetch is open.
    for (let round = 0; round < 20; round++) {
      for (let i = 0; i < 50; i++) requests.push(coalescer.request());
      await tick();
      assert(backend.maxInFlight() === 1, `max in flight hit ${backend.maxInFlight()}`);
      assert(backend.open() === 1, `expected exactly 1 open fetch, got ${backend.open()}`);
      backend.releaseNext();
      await tick();
    }
    while (backend.open() > 0) {
      backend.releaseNext();
      await tick();
    }
    await Promise.all(requests);
    assert(backend.maxInFlight() === 1, `max in flight was ${backend.maxInFlight()}`);
    assert(
      backend.calls() <= 22,
      `1000 requests cost ${backend.calls()} fetches; the bound is one per round plus one`,
    );
  });

  await test("no waiter is answered by a fetch that started before its request", async () => {
    let started = 0;
    let lastFinished = 0;
    const releases: Array<() => void> = [];
    const run = (): Promise<boolean> => {
      const mine = ++started;
      return new Promise<boolean>((resolve) => {
        releases.push(() => {
          lastFinished = mine;
          resolve(true);
        });
      });
    };
    const coalescer = createRefreshCoalescer(run);
    const first = coalescer.request();
    await tick();
    const openWhenRequested = started; // fetch 1 is open
    let answeredBy = 0;
    const late = coalescer.request().then(() => { answeredBy = lastFinished; });
    releases.shift()!();
    await first;
    await tick();
    releases.shift()!();
    await late;
    assert(
      answeredBy > openWhenRequested,
      `a request made during fetch ${openWhenRequested} was answered by fetch ${answeredBy}`,
    );
  });

  await test("a failing fetch does not wedge the coalescer", async () => {
    let calls = 0;
    const coalescer = createRefreshCoalescer(async () => {
      calls++;
      if (calls === 1) throw new Error("backend down");
      return true;
    });
    assert((await coalescer.request()) === false, "a thrown fetch resolves false, not a rejection");
    assert((await coalescer.request()) === true, "the coalescer keeps serving after a failure");
    assert(coalescer.inFlight() === 0, "nothing left in flight");
  });

  await test("sequential requests each get their own fetch", async () => {
    let calls = 0;
    const coalescer = createRefreshCoalescer(async () => {
      calls++;
      return true;
    });
    await coalescer.request();
    await coalescer.request();
    await coalescer.request();
    assert(calls === 3, `expected 3 sequential fetches, got ${calls}`);
  });

  console.log(failures === 0 ? "\nall passed" : `\n${failures} failed`);
  if (failures > 0) process.exitCode = 1;
})();
