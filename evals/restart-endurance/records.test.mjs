import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { nextTurnModel } from "./records.mjs";

const marker = "test-model-turn";
function withLines(provider, rows, run) {
  const dir = mkdtempSync(join(tmpdir(), "endure-record-"));
  const path = join(dir, "record.jsonl");
  writeFileSync(path, rows.map((row) => JSON.stringify(row)).join("\n") + "\n");
  try { run({ provider, session: "test-session", path, bytes: 0 }); }
  finally { rmSync(dir, { recursive: true }); }
}

test("Claude proof requires the own appended user boundary", () => {
  withLines("claude", [
    { type: "assistant", message: { model: "old", content: marker } },
    { type: "user", message: { content: marker } },
    { type: "assistant", message: { model: "new", content: marker }, timestamp: "done" },
  ], (boundary) => {
    assert.equal(nextTurnModel(boundary, marker).model, "new");
    assert.equal(nextTurnModel(boundary, "absent"), null);
  });
});

test("Codex proof comes from the completed appended rollout turn", () => {
  withLines("codex", [
    { type: "turn_context", payload: { model: "new" } },
    { type: "response_item", payload: { role: "user", content: marker } },
    { type: "response_item", payload: { role: "assistant", content: marker } },
    { type: "event_msg", payload: { type: "task_complete" }, timestamp: "done" },
  ], (boundary) => assert.equal(nextTurnModel(boundary, marker).model, "new"));
});

test("Grok proof takes the model from the own completed turn", () => {
  withLines("grok", [
    { params: { update: { sessionUpdate: "user_message_chunk", content: { text: marker }, _meta: { modelId: "new" } } } },
    { params: { update: { sessionUpdate: "agent_message_chunk", content: { text: marker } } } },
    { params: { update: { sessionUpdate: "turn_completed" } } },
  ], (boundary) => assert.equal(nextTurnModel(boundary, marker).model, "new"));
});

test("OpenCode proof excludes baseline messages and requires completion", () => {
  const dir = mkdtempSync(join(tmpdir(), "endure-sqlite-"));
  const path = join(dir, "opencode.db");
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT); CREATE TABLE part (message_id TEXT, time_created INTEGER, data TEXT)");
  db.prepare("INSERT INTO message VALUES (?, ?, ?, ?)").run("new-message", "own", 1, JSON.stringify({role: "assistant", providerID: "test", modelID: "new", time: {completed: 2}}));
  db.prepare("INSERT INTO part VALUES (?, ?, ?)").run("new-message", 1, JSON.stringify({type: "text", text: marker}));
  db.close();
  try {
    const boundary = {provider: "opencode", session: "own", path, message_ids: []};
    assert.equal(nextTurnModel(boundary, marker).model, "test/new");
    assert.equal(nextTurnModel({...boundary, message_ids: ["new-message"]}, marker), null);
    assert.equal(nextTurnModel({...boundary, session: "foreign"}, marker), null);
  } finally { rmSync(dir, {recursive: true}); }
});
