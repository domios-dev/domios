import assert from "node:assert/strict";
import { test } from "node:test";
import { turnEvidence, switchModels, type Descriptor, type Evidence, type RawRecord } from "./live-model-switch";
import { detectChatPrefix } from "../../../src/lib/chatPrefixes";

const evidence = (records: RawRecord[]): Evidence => ({ path: "owned-records", session: "owned", records });

test("effort matrix avoids a default-only model when an explicit pair exists", () => {
  const model = (id: string, efforts: string[]) => ({ id, display_name: id, reasoning_efforts: efforts.map(id => ({ id })) });
  const descriptor: Descriptor = { id: "opaque", default_model: "default-only", model_switch: "next_turn", reasoning_efforts: [],
    available_models: [model("default-only", ["default"]), model("a", ["default", "low", "high"]), model("b", ["low", "high"])] };
  assert.deepEqual(switchModels(descriptor), { a: descriptor.available_models[1], b: descriptor.available_models[2] });
  descriptor.default_model = "b";
  assert.equal(switchModels(descriptor).a.id, "b");
});

test("Codex model and effort come from the retained user's exact turn_id", () => {
  const rows = evidence([
    { type: "turn_context", payload: { turn_id: "foreign", model: "wrong", effort: "max" } },
    { type: "turn_context", payload: { turn_id: "own-turn", model: "new", effort: "low" } },
    { type: "response_item", payload: { role: "user", content: [{ text: "opaque-marker" }] }, metadata: { retained_source: { id: { turn_id: "own-turn" } } } },
  ]);
  assert.equal(turnEvidence("codex", rows, "opaque-marker").model, "new");
  assert.equal(turnEvidence("codex", rows, "opaque-marker").effort, "low");
  rows.records[1].payload!.turn_id = "other";
  assert.throws(() => turnEvidence("codex", rows, "opaque-marker"), /No Codex turn_context/);
});

test("Claude requestedModel cannot substitute for an assistant's model", () => {
  const rows = evidence([{ type: "user", message: { content: "opaque-marker" } }]);
  assert.throws(() => turnEvidence("claude", rows, "opaque-marker"), /No assistant model/);
  rows.records.push({ type: "assistant", message: { model: "actual" }, effort: "high" });
  assert.equal(turnEvidence("claude", rows, "opaque-marker").model, "actual");
  assert.equal(turnEvidence("claude", rows, "opaque-marker").effort, "high");
});

test("Grok requires own-session assistant-linked completion usage, not user model metadata alone", () => {
  const rows = evidence([
    { params: { sessionId: "owned", update: { sessionUpdate: "user_message_chunk", content: "opaque-marker", _meta: { modelId: "actual" } } } },
    { params: { sessionId: "owned", update: { sessionUpdate: "agent_message_chunk" }, _meta: { promptId: "own-prompt" } } },
    { params: { sessionId: "owned", update: { sessionUpdate: "turn_completed", prompt_id: "own-prompt", usage: { modelUsage: { actual: {} } } } } },
  ]);
  assert.equal(turnEvidence("grok", rows, "opaque-marker").model, "actual");
  rows.records[2].params!.update!.usage!.modelUsage = { wrong: {} };
  assert.throws(() => turnEvidence("grok", rows, "opaque-marker"), /No Grok turn model/);
  rows.records[2].params!.update!.usage!.modelUsage = { actual: {} };
  rows.records[0].params!.sessionId = "foreign";
  assert.throws(() => turnEvidence("grok", rows, "opaque-marker"), /No Grok turn model/);
});

test("OpenCode assistant is parent-linked to the marked user, not the newest global assistant", () => {
  const rows = evidence([
    { id: "u", role: "user", message: { content: "opaque-marker" } },
    { id: "a", role: "assistant", parentID: "foreign", modelID: "wrong", providerID: "p" },
  ]);
  assert.throws(() => turnEvidence("opencode", rows, "opaque-marker"), /exact parent-linked/);
  rows.records[0].variant = "low";
  rows.records.push({ id: "own-a", role: "assistant", parentID: "u", modelID: "actual", providerID: "p", time: { completed: 1 }, parts: [{ type: "text", text: "opaque-marker" }] });
  assert.equal(turnEvidence("opencode", rows, "opaque-marker").model, "p/actual");
  assert.equal(turnEvidence("opencode", rows, "opaque-marker").effort, "low");
  rows.records[2].time = {};
  assert.throws(() => turnEvidence("opencode", rows, "opaque-marker"), /exact parent-linked/);
});

test("command wrapper classification preserves stray user text", () => {
  const wrapper = "<local-command-stdout>opaque-command-output</local-command-stdout>";
  assert.equal(detectChatPrefix(wrapper)?.body, "");
  assert.equal(detectChatPrefix(`${wrapper}\nopaque-user-text`)?.body, "opaque-user-text");
});
