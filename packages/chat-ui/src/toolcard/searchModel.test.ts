import assert from "node:assert/strict";
import type { ToolEvent } from "@codeterm/chat-core";
import { searchModel } from "./searchModel";

function event(overrides: Partial<ToolEvent> = {}): ToolEvent {
  return { id: "search", kind: "search", phase: "result", status: "success", summary: "Search matches", message: { id: "search", type: "tool_call", content: "", toolName: "Grep", toolInput: { pattern: "alpha", path: "a.ts" } }, ...overrides };
}

const single = searchModel(event({ result: "12:alpha" }));
assert.equal(single.subject, "alpha");
assert.equal(single.matches, 1);
assert.equal(single.files, undefined);
assert.equal(single.counts, "1 match");
assert.equal(searchModel(event({ result: "a.ts:1:alpha\nb.ts:2:alpha" })).counts, "2 matches · 2 files");
assert.equal(searchModel(event({ result: "a.ts:1:alpha\na.ts:2:alpha" })).counts, "2 matches · 1 file");
assert.equal(searchModel(event({ summary: "1 match · 0 files", result: "12:alpha" })).counts, "1 match");
assert.equal(searchModel(event({ summary: "3 matches in 2 files", result: "a\nb\nc" })).counts, "3 matches · 2 files");
assert.equal(searchModel(event({ summary: "1 match in 1 file" })).counts, "1 match · 1 file");
assert.equal(searchModel(event({ result: "" })).counts, "0 matches");
assert.equal(searchModel(event({ status: "pending" })).counts, "");

const globEvent = event({ message: { id: "glob", type: "tool_call", content: "", toolName: "Glob", toolArgs: JSON.stringify({ pattern: "visual-stream*" }) }, result: "visual-stream.ts" });
const glob = searchModel(globEvent);
assert.equal(glob.title, "Find files");
assert.equal(glob.subject, "visual-stream*");
assert.equal(glob.files, 1);
assert.equal(glob.counts, "1 file");
assert.equal(searchModel({ ...globEvent, result: "a.ts\nb.ts" }).counts, "2 files");
assert.equal(searchModel({ ...globEvent, result: "" }).files, undefined);

for (const command of ["rg -n alpha a.ts", "grep -n 'alpha beta' a.ts", "rg -g '*.ts' -e 'alpha beta' src"]) {
  const model = searchModel(event({ message: { id: "shell", type: "tool_call", content: "", toolInput: { command } }, result: "1:alpha" }));
  assert.equal(model.subject, command.includes("beta") ? "alpha beta" : "alpha");
  assert.equal(model.title, "Search");
}
const fileSearch = searchModel(event({ message: { id: "shell", type: "tool_call", content: "", toolInput: { command: "rg --files -g 'visual-stream*'" } }, result: "visual-stream.ts" }));
assert.equal(fileSearch.subject, "visual-stream*");
assert.equal(fileSearch.title, "Find files");
assert.equal(fileSearch.counts, "1 file");
console.log("search model tests: passed");
