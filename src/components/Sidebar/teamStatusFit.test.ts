import assert from "node:assert/strict";
import { compactTeamStatus } from "./teamStatusFit";

assert.equal(compactTeamStatus(180, 200), false);
assert.equal(compactTeamStatus(180, 180), false);
assert.equal(compactTeamStatus(180.25, 180), false);
assert.equal(compactTeamStatus(181, 180), true);
assert.equal(compactTeamStatus(220, 200), true);
assert.equal(compactTeamStatus(100, 0), false);
assert.equal(compactTeamStatus(Number.NaN, 200), false);
console.log("Team status fit: 7 checks passed");
