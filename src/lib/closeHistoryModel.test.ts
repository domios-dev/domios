import assert from "node:assert/strict";
import { describeRestore, restorableStructures } from "./closeHistoryModel";
import type { ClosedStructureView } from "./types";

const first: ClosedStructureView = {
  id: "close-1", kind: "team", label: "Editors", state: "closed", closed_at: 100,
  undo_until: 700, member_count: 4, team_count: 1, shell_count: 1, agent_count: 3,
  cron_count: 1, blueprint_count: 1,
};
const next = { ...first, id: "close-2", kind: "workspace" as const, closed_at: 300 };
assert.deepEqual(restorableStructures([first, { ...next, state: "restored" }]), [first]);
assert.ok(describeRestore(first).includes(String(first.member_count)));
console.log("closeHistoryModel passed");
