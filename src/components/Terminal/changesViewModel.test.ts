import assert from "node:assert/strict";
import { sessionChanges } from "@codeterm/chat-core";
import { changeBar, changeContext, changeReference, changesFilePath, changesTotals, diffCapsuleLabel, fenced, hunkCounts, hunkReference, scopedFiles, turnNumber, turnOptions, unifiedDiffText } from "./changesViewModel";

const changes = sessionChanges([
  { id: "turn1", type: "user", content: "first", timestamp: "2026-10-08T10:00:00Z" },
  { id: "edit", type: "tool_result", content: "", toolEdits: [
    { path: "z\\two.ts", old: "x", new: "y\nz" },
    { path: "a/one.ts", old: "", new: "one" },
  ] },
  { id: "turn2", type: "user", content: "no edits" },
]);
const files = changes.cumulative;
assert.deepEqual(changesTotals(files), { additions: 3, deletions: 1 });
assert.equal(changesFilePath(files[1], "/repo/"), "/repo/a/one.ts");
assert.equal(changesFilePath(files[1]), null);
assert.equal(changesFilePath({ ...files[1], movePath: "C:\\repo\\moved.ts" }, "/ignored"), "C:\\repo\\moved.ts");
assert.equal(changesFilePath({ ...files[1], path: "/absolute.ts" }), "/absolute.ts");

assert.deepEqual(changeBar(0, 0), ["none", "none", "none", "none", "none"]);
assert.deepEqual(changeBar(38, 0), ["add", "add", "add", "add", "add"]);
assert.deepEqual(changeBar(0, 9), ["del", "del", "del", "del", "del"]);
assert.deepEqual(changeBar(100, 1), ["add", "add", "add", "add", "del"]);
assert.deepEqual(changeBar(1, 100), ["add", "del", "del", "del", "del"]);
assert.deepEqual(changeBar(3, 2), ["add", "add", "add", "del", "del"]);

assert.equal(scopedFiles(changes, "all"), files);
assert.equal(scopedFiles(changes, { turnId: "turn1" }).length, 2);
assert.deepEqual(scopedFiles(changes, { turnId: "missing" }), []);
const options = turnOptions(changes.turns, () => "10:00");
assert.deepEqual(options.map(({ turnId, label, detail }) => [turnId, label, detail]), [["turn1", "Turn 1", "10:00, 2 files"]]);
assert.equal(turnNumber(changes.turns, "turn2"), 2);
assert.equal(turnNumber(changes.turns, "nope"), null);

const one = files.find((file) => file.path === "a/one.ts")!;
const two = files.find((file) => file.path === "z\\two.ts")!;
const twoDiff = { ...two, diffs: [{ ...two.diffs[0], path: "z/two.ts" }] };
assert.equal(unifiedDiffText(twoDiff), "--- a/z/two.ts\n+++ b/z/two.ts\n@@ -1,1 +1,2 @@\n-x\n+y\n+z");
assert.equal(unifiedDiffText({ ...one, diffs: [{ ...one.diffs[0], kind: "add" }] }).split("\n")[0], "--- /dev/null");
assert.equal(unifiedDiffText({ ...one, diffs: [{ ...one.diffs[0], kind: "delete" }] }).split("\n")[1], "+++ /dev/null");
assert.equal(fenced("a ``` b", "diff"), "````diff\na ``` b\n````");
assert.equal(hunkReference("z/two.ts", two.diffs[0].hunks[0]), "Recorded change in `z/two.ts:1-2`:\n```diff\n@@ -1,1 +1,2 @@\n-x\n+y\n+z\n```");
assert.equal(changeReference("z/two.ts", two), "Recorded change in `z/two.ts` (modified, +2 -1; @@ -1,1 +1,2 @@)");
const many = { ...two, diffs: [{ ...two.diffs[0], hunks: Array(5).fill(two.diffs[0].hunks[0]) }] };
assert.match(changeReference("z/two.ts", many, 2), /and 3 more\)$/);
assert.deepEqual(hunkCounts(two.diffs[0].hunks[0]), { additions: 2, deletions: 1 });
assert.equal(diffCapsuleLabel("src\\ChangesPanel.tsx", { additions: 26, deletions: 4 }), "diff · ChangesPanel.tsx +26 −4");
assert.equal(changeContext("z/two.ts", twoDiff, "/w/z/two.ts"),
  "Recorded change in `z/two.ts` (modified, +2 -1; @@ -1,1 +1,2 @@)\n\nCurrent file: `/w/z/two.ts`\n\n```diff\n--- a/z/two.ts\n+++ b/z/two.ts\n@@ -1,1 +1,2 @@\n-x\n+y\n+z\n```");
assert.ok(!changeContext("z/two.ts", twoDiff, null).includes("Current file"));
console.log("Changes view presentation passed");
