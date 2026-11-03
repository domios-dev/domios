import assert from "node:assert/strict";
import { acceptViewportFrame, completeResize, requestResize } from "./resizeFrameModel";
import { createViewportResizer } from "./viewportResizer";

const a = { w: 640, h: 480, dpr: 1 };
const b = { w: 800, h: 600, dpr: 2 };
const c = { w: 900, h: 700, dpr: 2 };
const started = requestResize({ inFlight: null, pending: null }, a);
assert.deepEqual(started, { inFlight: a, pending: null });
const superseded = requestResize(requestResize(started, b), c);
assert.deepEqual(superseded, { inFlight: a, pending: c });
assert.deepEqual(completeResize(superseded), { inFlight: c, pending: null });
assert.deepEqual(completeResize(started), { inFlight: null, pending: null });
assert.deepEqual(requestResize(superseded, a), started, "returning to the in-flight size cancels obsolete pending work");
assert.equal(acceptViewportFrame({ deviceWidth: 640, deviceHeight: 480 }, a), true);
assert.equal(acceptViewportFrame({ deviceWidth: 800, deviceHeight: 600 }, a), false);
assert.equal(acceptViewportFrame({ deviceWidth: 640, deviceHeight: 600 }, a), false);
assert.equal(acceptViewportFrame({ deviceWidth: 320, deviceHeight: 240 }, { w: 200, h: 100, dpr: 3 }), true, "backend minimum dimensions apply");
assert.equal(acceptViewportFrame({ deviceWidth: 640, deviceHeight: 480 }, null), false);

async function runQueueTests() {
  const calls: typeof a[] = [];
  const acknowledgements: Array<() => void> = [];
  const queue = createViewportResizer((viewport) => {
    calls.push(viewport);
    return new Promise<void>((resolve) => acknowledgements.push(resolve));
  }, () => assert.fail("unexpected resize failure"));
  queue.push(a);
  queue.push(b);
  queue.push(c);
  assert.deepEqual(calls, [a], "only one RPC may be in flight");
  acknowledgements.shift()!();
  await Promise.resolve();
  assert.deepEqual(calls, [a, c], "acknowledgement starts only the newest pending resize");
  queue.push(b);
  queue.dispose();
  acknowledgements.shift()!();
  await Promise.resolve();
  assert.deepEqual(calls, [a, c], "disposal drops pending work");
  console.log("✓ resizeFrameModel: 14 model and queue assertions green");
}

runQueueTests().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
