import assert from "node:assert/strict";
import { describeRestore, restorableStructures, undoableClose } from "./closeHistoryModel";
import type { ClosedStructureView } from "./types";

const first: ClosedStructureView = {
  id: "close-1", kind: "team", label: "Editors", state: "closed", closed_at: 100,
  undo_until: 700, member_count: 4, team_count: 1, shell_count: 1, agent_count: 3,
  cron_count: 1, blueprint_count: 1,
};
assert.equal(undoableClose([first], 699)?.id, first.id);
assert.equal(undoableClose([first], 701), null);
assert.equal(undoableClose([{ ...first, state: "restoring" }], 200), null);
assert.equal(undoableClose([{ ...first, state: "restored" }], 200), null);
const next = { ...first, id: "close-2", kind: "workspace" as const, closed_at: 300 };
assert.equal(undoableClose([first, next], 400)?.id, next.id);
assert.equal(undoableClose([{ ...next, state: "restored" }, first], 400), null);
assert.deepEqual(restorableStructures([first, { ...next, state: "restored" }]), [first]);
assert.equal(undoableClose(JSON.parse(JSON.stringify([next, first])), 400)?.id, next.id);
assert.ok(describeRestore(first).includes(String(first.member_count)));
console.log("closeHistoryModel passed");
