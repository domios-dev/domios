/**
 * Claude structuredPatch results normalize to the provider-neutral file_diffs capability.
 * Run: npx tsx plugins/provider-claude/tests/file-diffs.test.ts
 */

import assert from "node:assert/strict";
import fs from "node:fs";
import { claudeFileDiffs } from "../src/fileDiffs";

interface Block { kind: string; data: Record<string, unknown> }
interface Row { role: string; blocks: Block[] }
interface Parser { parseSessionDelta(chunk: string): { messages: Row[] } }

(globalThis as { host?: unknown }).host ??= { manifestJson: () => "null", log: () => undefined };
const parser = (await import("../src/plugin")).default as unknown as Parser;

const raw = fs.readFileSync(new URL("./fixtures/file-diffs.jsonl", import.meta.url), "utf8");
const sourceRows = raw.split("\n").filter(Boolean).flatMap((line) => {
  try { return [JSON.parse(line) as { toolUseResult?: { structuredPatch?: { lines: string[] }[] } }]; } catch (_) { return []; }
});

function parse(chunk: string): Row[] {
  const originalWarn = console.warn;
  let warned = 0;
  try {
    console.warn = () => { warned++; };
    return JSON.parse(JSON.stringify(parser.parseSessionDelta(chunk))).messages;
  } finally {
    console.warn = originalWarn;
    assert.ok(warned <= 1, "a malformed line warns at most once");
  }
}

const rows = parse(raw);
assert.equal(rows.length, 11, "the malformed line is skipped and every other record parses");
assert.equal(rows[10].blocks[0].data.text, "Both files are updated.", "records after the malformed line stay usable");

const resultData = (index: number) => rows[index].blocks[0].data;
assert.equal(rows[1].blocks[0].kind, "tool_result");
assert.deepEqual(resultData(1).file_diffs, [{
  path: "src/shared.ts",
  additions: 1,
  deletions: 1,
  hunks: [{ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3, lines: [" a", "-b", "+c", " d"] }],
}], "an Edit result keeps its exact path, counts, and hunk");
assert.deepEqual(
  (resultData(1).file_diffs as { hunks: { lines: string[] }[] }[])[0].hunks[0].lines,
  sourceRows[1].toolUseResult?.structuredPatch?.[0].lines,
  "hunk lines are the provider's own lines",
);
assert.deepEqual(resultData(3).file_diffs, [{
  path: "src/created.ts",
  additions: 2,
  deletions: 0,
  hunks: [{ oldStart: 0, oldLines: 0, newStart: 1, newLines: 2, lines: ["+x", "+y"] }],
}], "a created file's exact content is its added side");

assert.equal(resultData(5).tool_use_id, "toolu_edit_bad");
assert.equal(resultData(5).file_diffs, undefined, "a hunk line without a marker skips only the diff capability");
assert.deepEqual(rows[4].blocks[0].data.tool_edits, [{ path: "src/broken.ts", old: "q", new: "r" }], "the malformed file's ordinary edit row is intact");
assert.equal(resultData(7).file_diffs, undefined, "an empty filePath cannot identify a file");
assert.equal(resultData(9).file_diffs, undefined, "a non-edit result carries no diff");
assert.equal(rows[0].blocks[0].data.file_diffs, undefined, "calls never carry a result's diff");

const twoResults = parse(JSON.stringify({
  type: "user",
  message: { role: "user", content: [
    { type: "tool_result", tool_use_id: "toolu_one", content: "one" },
    { type: "tool_result", tool_use_id: "toolu_two", content: "two" },
  ] },
  toolUseResult: { filePath: "src/x.ts", structuredPatch: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-a", "+b"] }] },
}));
assert.deepEqual(twoResults[0].blocks.map((block) => block.data.file_diffs), [undefined, undefined], "several results in one row leave the diff unlinked");

assert.deepEqual(claudeFileDiffs(null), []);
assert.deepEqual(claudeFileDiffs({ filePath: "src/x.ts" }), [], "a result without structuredPatch has no diff");
assert.deepEqual(claudeFileDiffs({ type: "update", filePath: "src/x.ts", structuredPatch: [] }), [], "an empty patch on an update is not a diff");
assert.deepEqual(claudeFileDiffs({ filePath: "src/x.ts", structuredPatch: [{ oldStart: "1", oldLines: 1, newStart: 1, newLines: 1, lines: ["+a"] }] }), [], "non-numeric hunk ranges are malformed");
assert.deepEqual(claudeFileDiffs({ type: "create", filePath: "src/empty.ts", content: "", structuredPatch: [] }), [{ path: "src/empty.ts", additions: 0, deletions: 0, hunks: [] }]);

console.log("Claude file diff fixtures passed");
