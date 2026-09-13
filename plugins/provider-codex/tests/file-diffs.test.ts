/**
 * Codex FileChange `changes` maps normalize to the provider-neutral file_diffs capability.
 * Run: npx tsx plugins/provider-codex/tests/file-diffs.test.ts
 */

import assert from "node:assert/strict";
import fs from "node:fs";
import plugin from "../src/plugin";
import { codexFileDiffs } from "../src/fileDiffs";

interface Block { kind: string; data: Record<string, unknown> }
interface Row { role: string; blocks: Block[] }
interface Parser { parseSessionDelta(chunk: string): { messages: Row[] } }

const parser = plugin as unknown as Parser;
const raw = fs.readFileSync(new URL("./fixtures/file-diffs.jsonl", import.meta.url), "utf8");

const originalWarn = console.warn;
let rows: Row[];
try {
  console.warn = () => undefined;
  rows = JSON.parse(JSON.stringify(parser.parseSessionDelta(raw))).messages;
} finally {
  console.warn = originalWarn;
}

assert.deepEqual(rows.map((row) => row.blocks.map((block) => block.kind)), [
  ["tool_use", "tool_result"],
  ["tool_use", "tool_result"],
  ["tool_use"],
  ["tool_result"],
  ["text"],
], "the malformed line and change-less items are skipped while every other record parses");
assert.equal(rows[4].blocks[0].data.text, "Both files are updated.");

const change = rows[0].blocks[0].data;
assert.deepEqual(change.file_diffs, [
  { path: "src/shared.ts", additions: 1, deletions: 1, hunks: [{ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3, lines: [" a", "-b", "+c", " d"] }] },
  { path: "src/created.ts", additions: 2, deletions: 0, hunks: [{ oldStart: 0, oldLines: 0, newStart: 1, newLines: 2, lines: ["+x", "+y"] }] },
], "each changed file keeps its exact path, counts, and hunks");
assert.equal(change.id, "exec-file-change-a");
assert.equal(change.tool_kind, "edit");
assert.equal(change.tool_summary, "src/shared.ts +1");
assert.equal(change.tool_title, "Patched shared.ts +1");
assert.equal(change.tool_flow_id, "turn-diff");
assert.equal(change.tool_status, "success");
assert.equal(change.tool_echo, undefined, "a file change is the only record carrying its diff, so it is never an echo");
assert.deepEqual(change.tool_edits, [
  { path: "src/shared.ts", old: "b", new: "c" },
  { path: "src/created.ts", old: "", new: "x\ny" },
]);
assert.deepEqual(rows[0].blocks[1].data, {
  tool_use_id: "exec-file-change-a",
  content: "Success. Updated the following files:\nM src/shared.ts\nA src/created.ts\n",
  is_error: false,
});

const partial = rows[1].blocks[0].data;
assert.deepEqual(partial.file_diffs, [
  { path: "src/removed.ts", additions: 0, deletions: 1, hunks: [{ oldStart: 1, oldLines: 1, newStart: 0, newLines: 0, lines: ["-gone"] }] },
], "malformed, unknown, and path-less entries drop only their own file");
assert.equal(partial.tool_summary, "src/broken.ts +3", "the ordinary action still names every identified file");
assert.deepEqual((partial.input as { paths: string[] }).paths, ["src/broken.ts", "src/removed.ts", "src/unknown.ts", "src/typeless.ts"]);

assert.equal(rows[2].blocks[0].data.id, "call_patch_legacy");
assert.deepEqual(rows[3].blocks[0].data.file_diffs, [
  { path: "src/legacy.ts", movePath: "src/renamed.ts", additions: 1, deletions: 1, hunks: [{ oldStart: 2, oldLines: 1, newStart: 2, newLines: 1, lines: ["-old", "+new"] }] },
], "a patch_apply_end changes map rides on the result linked to its call");
assert.equal(rows[3].blocks[0].data.tool_use_id, "call_patch_legacy");

assert.deepEqual(codexFileDiffs(null), []);
assert.deepEqual(codexFileDiffs([]), [], "an array is not a changes map");
assert.deepEqual(codexFileDiffs({ "src/a.ts": { type: "update", unified_diff: "@@ -1 +1 @@\n-a\n\n+b\n" } }), [], "a bare empty line inside a hunk is malformed");
assert.deepEqual(codexFileDiffs({ "src/a.ts": { type: "update", unified_diff: "--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1 +1 @@\n-a\n+b\n\\ No newline at end of file\n" } }), [
  { path: "src/a.ts", additions: 1, deletions: 1, hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-a", "+b", "\\ No newline at end of file"] }] },
], "file headers before the first hunk and no-newline markers are accepted");
assert.deepEqual(codexFileDiffs({ "src/crlf.ts": { type: "update", unified_diff: "@@ -1 +1 @@\n-a\r\n+b\n" } })[0].hunks[0].lines, ["-a\r", "+b"], "carriage returns stay in the exact lines");
assert.deepEqual(codexFileDiffs({ "src/moved.ts": { type: "update", unified_diff: "", move_path: "src/new.ts" } }), [], "a move without hunks has no diff");

console.log("Codex file diff fixtures passed");
