import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { partToChatMsg } from "../src/helpers";
import { opencodeFileDiffs } from "../src/fileDiffs";
import { sessionChanges, type NormalizedChatMessage } from "@codeterm/chat-core";

const completed = partToChatMsg({ type: "tool", tool: "edit", callID: "call1", state: {
  status: "completed", input: { filePath: "/repo/a.ts", oldString: "old", newString: "new\nextra" }, output: "ok",
} }, "assistant", "part1", 1780000000000) as NormalizedChatMessage;
assert.equal(completed.toolId, "call1");
assert.equal(completed.toolError, false);
assert.deepEqual(completed.fileDiffs?.[0], { path: "/repo/a.ts", additions: 2, deletions: 1,
  hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 2, lines: ["-old", "+new", "+extra"] }] });
assert.equal(sessionChanges([completed]).cumulative[0].additions, 2, "the plugin output feeds the shared model without a provider switch");

const patch = "--- a/a.ts\n+++ b/a.ts\n@@ -5,2 +5,2 @@\n context\n-old\n+new\n";
const diff = opencodeFileDiffs("a.ts", [], { diff: patch })[0];
assert.deepEqual([diff.hunks[0].oldStart, diff.additions, diff.deletions, diff.truncated], [5, 1, 1, false]);
assert.deepEqual(opencodeFileDiffs("a.ts", [], { patch }), [diff]);
assert.equal(opencodeFileDiffs("a.ts", [], { diff: "@@ -1,5 +1,5 @@\n-x\n+y" })[0].truncated, true);
assert.equal(opencodeFileDiffs("image.png", [], { diff: "Binary files a/image.png and b/image.png differ" })[0].binary, true);
assert.deepEqual(opencodeFileDiffs("a", [], { diff: "not a diff" }), []);
const multi = opencodeFileDiffs("", [], { patch: "--- /dev/null\n+++ b/new.ts\n@@ -0,0 +1,1 @@\n+hello\n--- a/gone.ts\n+++ /dev/null\n@@ -1,1 +0,0 @@\n-bye\n" });
assert.deepEqual(multi.map((file) => [file.path, file.kind, file.additions, file.deletions]), [["new.ts", "add", 1, 0], ["gone.ts", "delete", 0, 1]]);
assert.equal(opencodeFileDiffs("", [], { files: [{ filePath: "/repo/a", type: "delete", diff: "@@ -1 +0,0 @@\n-x" }] })[0].kind, "delete");
assert.equal(opencodeFileDiffs("image.png", [], { diff: "diff --git a/image.png b/image.png\nBinary files a/image.png and b/image.png differ" })[0].binary, true);

const failed = partToChatMsg({ type: "tool", tool: "edit", state: { status: "error",
  input: { filePath: "a.ts", oldString: "x", newString: "y" }, output: "failed" } }, "assistant", "fail", 1) as NormalizedChatMessage;
assert.equal(sessionChanges([failed]).cumulative.length, 0);
const pending = partToChatMsg({ type: "tool", tool: "edit", state: { status: "running",
  input: { filePath: "a.ts", oldString: "x", newString: "y" } } }, "assistant", "pending", 1) as NormalizedChatMessage;
assert.equal(sessionChanges([pending]).cumulative.length, 0);
console.log("OpenCode file diff mapping passed");

const fixtures = JSON.parse(readFileSync(new URL("../../../tests/fixtures/opencode-file-diffs.json", import.meta.url), "utf8")) as {
  name: string; input: Record<string, unknown>; metadata: Record<string, unknown>; expected: unknown;
}[];
for (const fixture of fixtures) {
  const result = partToChatMsg({ type: "tool", tool: "edit", state: {
    status: "completed", input: fixture.input, metadata: fixture.metadata,
  } }, "assistant", fixture.name, 1) as NormalizedChatMessage;
  assert.deepEqual(result.fileDiffs, fixture.expected, fixture.name);
}
