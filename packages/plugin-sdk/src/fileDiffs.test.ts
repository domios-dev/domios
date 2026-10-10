import assert from "node:assert/strict";
import { recordedPatchDiff, recordedTextDiff } from "./fileDiffs";
import { wellFormedFileDiffs } from "../../chat-core/src/fileDiffs";
import { sessionChanges } from "../../chat-core/src/sessionChanges";

const text = recordedTextDiff("src/hello.ts", "before\r\n", "after\r\n");
assert.deepEqual(text, [{ path: "src/hello.ts", additions: 1, deletions: 1, truncated: true,
  hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-before\r", "+after\r"] }] }]);
const patch = recordedPatchDiff("sample.txt", "@@ -1 +1 @@\n-before\n+after\n", "modify");
assert.deepEqual(patch, [{ path: "sample.txt", kind: "modify", additions: 1, deletions: 1,
  hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-before", "+after"] }] }]);
for (const diffs of [text, patch]) {
  assert.deepEqual(wellFormedFileDiffs(diffs), diffs);
  assert.equal(sessionChanges([{ id: "user", type: "user", content: "Edit" },
    { id: "edit", type: "tool_result", content: "", toolError: false, fileDiffs: diffs }]).turns[0].files.length, 1);
}
assert.deepEqual(recordedTextDiff("sample.txt", undefined, "after"), []);
assert.deepEqual(recordedTextDiff("", "before", "after"), []);
assert.deepEqual(recordedTextDiff("sample.txt", "same", "same"), []);
assert.deepEqual(recordedPatchDiff("sample.txt", "@@ -1 +1 @@\n-before\n", "modify"), []);
assert.deepEqual(recordedPatchDiff("sample.txt", "@@ -1 +1 @@\n-before\n\n+after\n", "modify"), []);
assert.deepEqual(recordedPatchDiff("empty.txt", "", "add"), [{ path: "empty.txt", kind: "add", additions: 0, deletions: 0, hunks: [] }]);
console.log("recorded diff helpers: schema validation and session Changes passed");
