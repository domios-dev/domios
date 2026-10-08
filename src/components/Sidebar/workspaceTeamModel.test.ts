// Run: npx tsx src/components/Sidebar/workspaceTeamModel.test.ts
import assert from "node:assert/strict";
import { partitionTeamMembers, type TeamTabMember } from "./workspaceTeamModel";
import type { Tab } from "../../lib/types";

function member(id: string, overrides: Partial<Tab> = {}, index = 0): TeamTabMember {
  return { tab: { id, title: id, team_id: "team", parent_tab_id: null, role: null, ...overrides } as Tab, index };
}

const surface = (id: string, owner: string | null): TeamTabMember =>
  member(id, {
    parent_tab_id: owner,
    backing: { kind: "surface", plugin_id: "dash", surface_id: "board", instance_id: id },
  });

const members = [
  member("orch", { role: "Manager", role_profile: "orchestrator" }),
  member("worker", { role: "Worker", parent_tab_id: "orch" }),
  member("shell", { spawned_by_tab_id: "worker" }),
  surface("board-worker", "worker"),
  surface("board-orch", "orch"),
  surface("board-orphan", "gone"),
];
const { terminals, workers, surfacesByOwner } = partitionTeamMembers(members);

assert.deepEqual(workers.map((m) => m.tab.id), ["worker"]);
assert.deepEqual(surfacesByOwner.get("worker")?.map((m) => m.tab.id), ["board-worker"]);
assert.deepEqual(surfacesByOwner.get("orch")?.map((m) => m.tab.id), ["board-orch"]);
assert.deepEqual(
  terminals.map((m) => m.tab.id),
  ["shell", "board-orphan"],
  "a surface whose owner is not a Team agent stays visible with the utility tabs",
);

console.log("workspaceTeamModel: passed");
