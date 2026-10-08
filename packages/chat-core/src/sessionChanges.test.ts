import assert from "node:assert/strict";
import { fileDiffFromEdit, sessionChanges } from "./sessionChanges";
import type { FileDiff, NormalizedChatMessage } from "./types";

const user = (id: string): NormalizedChatMessage => ({ id, type: "user", content: id, timestamp: "2026-10-08T12:00:00Z" });
const edit = (id: string, path: string, old: string, next: string, patch: Partial<FileDiff> = {}): NormalizedChatMessage => ({
  id, type: "tool_result", content: "", fileDiffs: [{ ...fileDiffFromEdit({ path, old, new: next }), ...patch }],
});

assert.deepEqual(sessionChanges([]), { turns: [], cumulative: [] });
const result = sessionChanges([
  user("turn1"), edit("a", "src/a.ts", "", "one\n", { kind: "add" }),
  user("turn2"), edit("b", "src/a.ts", "one", "two\nthree"),
]);
assert.deepEqual(result.turns.map((turn) => [turn.turnId, turn.files[0].additions, turn.files[0].deletions]), [["turn1", 1, 0], ["turn2", 2, 1]]);
assert.deepEqual(result.cumulative.map((file) => [file.path, file.kind, file.additions, file.deletions, file.diffs.length, file.turnIds]),
  [["src/a.ts", "add", 3, 1, 2, ["turn1", "turn2"]]]);
assert.equal(result.turns[0].files[0].diffs.length, 1, "cumulative folding cannot mutate a turn's list");

const renamed = sessionChanges([
  user("t1"), edit("r1", "old.ts", "old", "new", { movePath: "dir/middle.ts" }),
  user("t2"), edit("r2", "dir/middle.ts", "new", "next", { movePath: "dir/final.ts" }),
  user("t3"), edit("r3", "dir\\final.ts", "next", "last"),
]);
assert.equal(renamed.cumulative.length, 1);
assert.deepEqual([renamed.cumulative[0].path, renamed.cumulative[0].movePath, renamed.cumulative[0].kind], ["old.ts", "dir/final.ts", "rename"]);
assert.deepEqual(renamed.turns[2].files.map((file) => file.path), ["dir\\final.ts"]);
const reused = sessionChanges([user("t1"), edit("r1", "a.ts", "x", "y", { movePath: "b.ts" }),
  user("t2"), edit("add", "a.ts", "", "new", { kind: "add" })]);
assert.deepEqual(reused.cumulative.map((file) => [file.path, file.movePath, file.kind]), [["a.ts", "b.ts", "rename"], ["a.ts", undefined, "add"]]);

const deleted = sessionChanges([user("t"), edit("d", "gone.ts", "gone", "", { kind: "delete" })]);
assert.equal(deleted.cumulative[0].kind, "delete");
const recreated = sessionChanges([user("t1"), edit("d", "a", "old", "", { kind: "delete" }), user("t2"), edit("c", "a", "", "new", { kind: "add" })]);
assert.equal(recreated.cumulative[0].kind, "modify");

const special = sessionChanges([user("t"),
  { id: "bin", type: "tool_result", content: "", fileDiffs: [{ path: "photo.png", additions: 0, deletions: 0, hunks: [], binary: true }] },
  edit("cut", "partial.ts", "x", "y", { truncated: true }),
  edit("mismatch", "short.ts", "x", "y", { hunks: [{ oldStart: 1, oldLines: 5, newStart: 1, newLines: 5, lines: ["-x", "+y"] }] }),
]);
assert.equal(special.cumulative[0].binary, true);
assert.equal(special.cumulative[1].truncated, true);
assert.equal(special.cumulative[2].truncated, true);

const call: NormalizedChatMessage = { id: "call", type: "tool_call", content: "", toolId: "edit", toolEdits: [{ path: "a", old: "x", new: "y" }] };
assert.equal(sessionChanges([user("t"), call]).cumulative.length, 0, "an unconfirmed input is not a recorded edit");
assert.equal(sessionChanges([user("t"), call, { id: "fail", type: "tool_result", content: "", toolId: "edit", toolError: true }]).cumulative.length, 0);
const linked = sessionChanges([user("t"), call, { ...edit("result", "a", "x", "y"), toolId: "edit" }]);
assert.equal(linked.cumulative[0].additions, 1, "a call and its result count once, preferring supplied hunks");
const mixed = sessionChanges([user("t"), { ...edit("mixed", "a", "x", "y"), toolEdits: [
  { path: "a", old: "x", new: "y" }, { path: "b", old: "old", new: "new" },
] }]);
assert.deepEqual(mixed.cumulative.map((file) => [file.path, file.additions]), [["a", 1], ["b", 1]], "structured hunks suppress only their own fallback edits");
const fallback = sessionChanges([user("t"), { ...call, toolError: false }]);
assert.equal(fallback.cumulative[0].diffs[0].hunks[0].lines.join("\n"), "-x\n+y");
const duplicate = edit("once", "a", "x", "y");
assert.equal(sessionChanges([user("t"), duplicate, duplicate]).cumulative[0].additions, 1);
assert.equal(sessionChanges([{ ...user("render-key"), sourceMessageId: "source-key" }, duplicate]).turns[0].turnId, "source-key");
assert.equal(sessionChanges([duplicate]).turns[0].turnId, "once", "a partial transcript retains edits preceding its first user turn");
console.log("sessionChanges folding passed");
