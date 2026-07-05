import {
  MISSION_CONTROL_FIXTURE,
  activeLaneCount,
  dispatchFrontierCount,
  totalBlockedParked,
} from "./fixtures";

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

const snap = MISSION_CONTROL_FIXTURE;

assert(snap.dispatchQueue.length >= 3, "dispatch frontier should have seeded queue");
assert(snap.lanes.length >= 3, "work floor should have multiple lane bays");
assert(snap.qaQueue.length >= 2 && snap.qaRunning, "QA bay needs queue + running slot");
assert(snap.gateQueue.length >= 1, "gate desk needs in_review tasks");
assert(snap.doneCount > snap.doneRecent.length, "done shelf is count-first");
assert(snap.parked.length >= 1 && snap.blocked.length >= 1, "side rail parked/blocked");
assert(snap.events.length >= 3, "event stream seeded");

assert(dispatchFrontierCount(snap) === snap.dispatchQueue.length, "dispatch count helper");
assert(activeLaneCount(snap) === 2, "two in_progress lane tasks in fixture");
assert(totalBlockedParked(snap) === snap.parked.length + snap.blocked.length, "blocked/parked total");

console.log("✓ missionControl fixtures");
