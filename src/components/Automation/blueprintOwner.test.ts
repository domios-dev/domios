import assert from "node:assert/strict";
import { runningBlueprintNamesForOwner } from "./blueprintOwnerModel";
import type { BlueprintSnapshot } from "./types";

const blueprints: BlueprintSnapshot[] = [
  { id: "tab-running", name: "Tab pipeline", owner: { kind: "tab", tab_id: "tab-a", team_id: null }, state: "running", run_id: "r1", current_step: 0, last_event_at_ms: 1 },
  { id: "team-running", name: "Team pipeline", owner: { kind: "team", tab_id: "tab-a", team_id: "team-a" }, state: "running", run_id: "r2", current_step: 0, last_event_at_ms: 2 },
  { id: "tab-idle", name: "Idle pipeline", owner: { kind: "tab", tab_id: "tab-a", team_id: null }, state: "idle", run_id: null, current_step: null, last_event_at_ms: null },
];

assert.deepEqual(runningBlueprintNamesForOwner(blueprints, "tab", "tab-a"), ["Tab pipeline"]);
assert.deepEqual(runningBlueprintNamesForOwner(blueprints, "team", "team-a"), ["Team pipeline"]);
assert.deepEqual(runningBlueprintNamesForOwner(blueprints, "tab", "missing"), []);

console.log("Blueprint owner close inventory cases passed");
