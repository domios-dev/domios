import assert from "node:assert/strict";
import { createSeqGapTracker } from "./eventSeqGaps";

const tracker = createSeqGapTracker();
tracker.adopt("boot", 1);
assert.equal(tracker.observe("boot", 3, 1).verdict, "ok");
assert.equal(tracker.observe("boot", 7, 3).verdict, "ok");
assert.equal(tracker.gaps(), 0);
assert.equal(tracker.observe("boot", 10, 9).verdict, "gap");
assert.equal(tracker.gaps(), 1);
assert.equal(tracker.observe("boot", 10, 9).verdict, "duplicate");
assert.equal(tracker.observe("boot", 7, 3).verdict, "duplicate");
assert.equal(tracker.observe("boot", 12, 10).verdict, "ok");
tracker.adopt("boot", 12);
assert.equal(tracker.observe("boot", 17, 12).verdict, "ok");
assert.equal(tracker.observe("boot", 19, 18).verdict, "gap");
assert.equal(tracker.observe("new", 100, null).verdict, "origin-changed");
tracker.adopt("new", 0);
assert.equal(tracker.observe("new", 1, 0).verdict, "ok");
assert.equal(tracker.observe("new", 3, null).verdict, "gap");
console.log("event sequence evidence: 13 assertions passed");

// A malformed overflow (non-numeric next_seq) must not poison the baseline: the next good frame reseeds it.
const reseeded = createSeqGapTracker();
reseeded.adopt("boot", 5);
reseeded.adopt("boot", Number.NaN);
reseeded.adopt("boot", undefined as unknown as number);
assert.equal(reseeded.observe("boot", 6, 5).verdict, "ok");
const fresh = createSeqGapTracker();
fresh.adopt("boot", Number.NaN);
assert.equal(fresh.observe("boot", 543, 540).verdict, "origin-changed");
assert.equal(fresh.observe("boot", 563, 543).verdict, "ok");
assert.equal(fresh.gaps(), 0);
