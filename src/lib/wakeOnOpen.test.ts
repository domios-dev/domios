import assert from "node:assert/strict";
import { shouldWakeOnOpen } from "./wakeOnOpen";

assert.equal(shouldWakeOnOpen({ frozen: true }, true, false), true);
assert.equal(shouldWakeOnOpen({ frozen: true }, false, false), false);
assert.equal(shouldWakeOnOpen({ frozen: true }, true, true), false);
assert.equal(shouldWakeOnOpen({ frozen: false }, true, false), false);
assert.equal(shouldWakeOnOpen({}, true, false), false);
assert.equal(shouldWakeOnOpen(undefined, true, false), false);
console.log("wake-on-open decision: 6 cases passed");
