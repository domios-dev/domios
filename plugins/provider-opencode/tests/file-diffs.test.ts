import assert from "node:assert/strict";
import vm from "node:vm";
import { readFileSync } from "node:fs";
import { partToChatMsg } from "../src/helpers";
import { opencodeFileDiffs } from "../src/fileDiffs";
import { materializeToolEvent, sessionChanges, type FileDiff, type NormalizedChatMessage } from "@codeterm/chat-core";

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

const fixtures = JSON.parse(readFileSync(new URL("../../../tests/fixtures/opencode-file-diffs.json", import.meta.url), "utf8")) as {
  name: string; input: Record<string, unknown>; metadata: Record<string, unknown>; expected: unknown;
}[];
for (const fixture of fixtures) {
  const result = partToChatMsg({ type: "tool", tool: "edit", state: {
    status: "completed", input: fixture.input, metadata: fixture.metadata,
  } }, "assistant", fixture.name, 1) as NormalizedChatMessage;
  assert.deepEqual(result.fileDiffs, fixture.expected, fixture.name);
}

const storedParts = JSON.parse(readFileSync(new URL("../../../tests/fixtures/opencode-1.18.35-tool-parts.json", import.meta.url), "utf8")) as Array<{
  name: string; part: Parameters<typeof partToChatMsg>[0]; expectedPath: string; additions: number; deletions: number;
}>;
const context = { module: { exports: {} }, host: {
  platform: () => "linux", envGet: () => null, expandHome: () => "/fixture",
  fs: { fileExists: () => true, sqliteQuery: () => storedParts.map((fixture, index) => ({
    id: fixture.name, ts_created: index + 1, ts_updated: index + 1, role: "assistant", part: JSON.stringify(fixture.part),
  })) },
} };
new vm.Script(readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
const bundled = (context.module.exports as { default: {
  structuredToolFileDiffs(part: unknown): FileDiff[] | null;
  readStructuredChat(cwd: string, sessionId: string): { messages: NormalizedChatMessage[] };
} }).default;
const history = JSON.parse(JSON.stringify(bundled.readStructuredChat("/repo", "own").messages)) as NormalizedChatMessage[];
for (const fixture of storedParts) {
  const liveDiffs = JSON.parse(JSON.stringify(bundled.structuredToolFileDiffs(fixture.part))) as FileDiff[];
  const row = history.find((message) => message.id === fixture.name)!;
  assert.deepEqual(row.fileDiffs, liveDiffs, "live and stored parts use the same plugin mapping");
  assert.deepEqual(liveDiffs.map((diff) => [diff.path, diff.additions, diff.deletions]),
    [[fixture.expectedPath, fixture.additions, fixture.deletions]]);
  assert.deepEqual(materializeToolEvent(row)?.fileDiffs, liveDiffs, "diff cards receive the recorded file diff");
  const changes = sessionChanges([row]).cumulative;
  assert.equal(changes.length, 1);
  assert.equal(changes[0].additions, fixture.additions);
  assert.equal(changes[0].deletions, fixture.deletions);
  for (const status of ["running", "pending", "error"]) {
    assert.equal(bundled.structuredToolFileDiffs({ ...fixture.part, state: { ...fixture.part.state, status } }), null);
  }
}
const write = storedParts[0].part;
assert.equal(bundled.structuredToolFileDiffs({ ...write, state: { ...write.state,
  metadata: { exists: true },
} })?.length, 0, "an overwrite without before text does not invent a full-file addition");
assert.equal(bundled.structuredToolFileDiffs({ ...write, state: { ...write.state,
  input: { filePath: "/repo/empty.txt", content: "" },
} })?.[0].kind, "add", "empty file creation remains a recorded change");

console.log("OpenCode file diff mapping passed");
