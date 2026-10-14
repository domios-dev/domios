import assert from "node:assert/strict";
import { streamLifecycleAction } from "./streamLifecycle";

const viewport = { w: 640, h: 480, dpr: 2 };
assert.equal(streamLifecycleAction(true, false, null, viewport), "connect");
assert.equal(streamLifecycleAction(false, true, viewport, viewport), "disconnect");
assert.equal(streamLifecycleAction(false, false, null, viewport), "none");
assert.equal(streamLifecycleAction(true, false, viewport, viewport), "connect", "show reconnects even at the same size");
assert.equal(streamLifecycleAction(true, true, viewport, viewport), "none");
assert.equal(streamLifecycleAction(true, true, viewport, { ...viewport, w: 800 }), "resize");
assert.equal(streamLifecycleAction(true, true, viewport, null), "disconnect", "collapsed stages suspend the cast");
console.log("✓ streamLifecycle tests green");
