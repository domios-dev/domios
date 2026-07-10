import assert from "node:assert/strict";
import { shouldRevealPresenceDetails } from "./presenceReveal";

const none = new Set<string>();

assert.equal(shouldRevealPresenceDetails("W0", "t1", none, false), false);
assert.equal(shouldRevealPresenceDetails("W1", "t1", none, false), false);
assert.equal(shouldRevealPresenceDetails("W2", "t1", none, false), true);
assert.equal(shouldRevealPresenceDetails("W0", "t1", new Set(["t1"]), false), true);
assert.equal(shouldRevealPresenceDetails("W0", "t1", none, true), true);

console.log("✓ presenceReveal tests passed");
