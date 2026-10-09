import assert from "node:assert/strict";
import { test } from "node:test";
import { completedTurn } from "./live-switch-completion";
import type { Evidence } from "./live-model-switch";

test("Claude model evidence without final marked assistant cannot release busy readiness", () => {
  const evidence: Evidence = { path: "own", session: "own", records: [
    { type: "user", message: { content: "marker" } },
    { type: "assistant", message: { model: "original", content: "marker", stop_reason: "tool_use" } },
  ] };
  assert.throws(() => completedTurn("claude", evidence, "marker"), /no completed assistant/);
  evidence.records[1].message!.stop_reason = "end_turn";
  evidence.records[1].message!.content = "foreign answer";
  assert.throws(() => completedTurn("claude", evidence, "marker"), /no completed assistant/);
  evidence.records[1].message!.content = "marker long answer";
  assert.equal(completedTurn("claude", evidence, "marker").model, "original");
});

test("Codex context alone or another turn's completion cannot release busy readiness", () => {
  const evidence: Evidence = { path: "own", session: "own", records: [
    { type: "response_item", payload: { role: "user", content: "marker" }, metadata: { retained_source: { id: { turn_id: "own-turn" } } } },
    { type: "turn_context", payload: { turn_id: "own-turn", model: "original" } },
    { type: "event_msg", payload: { type: "task_complete", turn_id: "foreign", last_agent_message: "marker" } },
  ] };
  assert.throws(() => completedTurn("codex", evidence, "marker"), /no exact task_complete/);
  evidence.records[2].payload!.turn_id = "own-turn";
  assert.equal(completedTurn("codex", evidence, "marker").model, "original");
});
