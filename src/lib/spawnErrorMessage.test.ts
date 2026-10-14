import assert from "node:assert/strict";
import { test } from "node:test";
import { isWorkspaceCwdRefusal, spawnErrorMessage } from "./spawnErrorMessage";
import { TransportError } from "./transportClient";

test("the error model preserves the backend human message across error envelopes", () => {
  const message = "owner supplied workspace/path recovery sentence";
  const body = JSON.stringify({ ok: false, error: { code: "workspace_cwd_missing", message } });
  for (const error of [
    new TransportError(422, "/api/agents/spawn", body),
    new TransportError(422, "/api/orchestration/new", body),
    JSON.parse(body), body, new Error(body), new Error("launch refused"),
  ]) {
    assert.equal(spawnErrorMessage(error), error instanceof Error && error.message === "launch refused"
      ? "launch refused" : message);
  }
});

test("truncated JSON and unknown values never become visible JSON", () => {
  for (const error of [
    new TransportError(500, "/api/agents/spawn", '{"error":{"class":"unknown"'),
    new Error('/api/agents/spawn → HTTP 500: {"error":'),
    { unexpected: "value" }, null,
  ]) {
    assert.equal(spawnErrorMessage(error).includes("{"), false);
    assert.notEqual(spawnErrorMessage(error), "[object Object]");
  }
});

test("workspace refusals are classified by the structured code for workspace-level handling", () => {
  assert.equal(isWorkspaceCwdRefusal(new TransportError(422, "/api/agents/spawn",
    JSON.stringify({ error: { code: "workspace_cwd_missing" } }))), true);
  assert.equal(isWorkspaceCwdRefusal(new Error("workspace_cwd_missing")), false);
  assert.equal(isWorkspaceCwdRefusal({ error: { code: "other" } }), false);
  assert.equal(spawnErrorMessage(new TransportError(422, "/api/workspaces/ws/cwd",
    "opaque plain-text refusal")), "opaque plain-text refusal");
});

test("a full transport body preserves long human messages and literal path braces", () => {
  const message = "Workspace {fixture} ".repeat(30);
  const error = new TransportError(422, "/api/agents/spawn", JSON.stringify({ error: { message } }));
  assert.equal(spawnErrorMessage(error), message);
});
