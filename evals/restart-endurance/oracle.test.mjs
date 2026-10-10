import assert from "node:assert/strict";
import test from "node:test";
import { exactReply, firstTurnPolicy, probeReady } from "./oracle.mjs";

const marker = "ENDURE_example_1";
const prompt = `Reply with exactly ${marker}`;
const user = { type: "user", content: prompt };
const assistant = { type: "assistant", content: marker };

test("own appended user turn and exact assistant reply pass", () => {
  assert.equal(exactReply([user, assistant], 0, prompt, marker), marker);
});
test("history, typed echo and assistant without the own user boundary fail", () => {
  assert.equal(exactReply([user, assistant], 2, prompt, marker), null);
  assert.equal(exactReply([user], 0, prompt, marker), null);
  assert.equal(exactReply([assistant], 0, prompt, marker), null);
});
test("extra response text and another user turn cannot satisfy the probe", () => {
  assert.equal(exactReply([user, assistant, { type: "assistant", content: "extra" }], 0, prompt, marker), null);
  assert.equal(exactReply([user, { type: "user", content: "unrelated" }, assistant], 0, prompt, marker), null);
});
test("tool output and thinking are not assistant answer evidence", () => {
  assert.equal(exactReply([user, { type: "tool_result", content: marker }], 0, prompt, marker), null);
  assert.equal(exactReply([user, { type: "thinking", content: marker }, assistant], 0, prompt, marker), marker);
});

test("declared lazy composer accepts first input without a session, then requires binding", () => {
  const policy = firstTurnPolicy({ spawn: { sessionStartsOnFirstUserTurn: true, providerReadiness: { kind: "structural_composer" } } });
  const record = { agent_type: "arbitrary-provider", boot_stage: "agent_ready", pty_id: "owned-pty", session_binding_nonce: "launch-nonce" };
  assert.equal(probeReady(record, policy), true);
  assert.equal(probeReady(record, policy, true), false);
  assert.equal(probeReady({ ...record, agent_session_id: "own-session" }, policy, true), true);
});
test("sessionless input refuses undeclared laziness, absent composer capability and pending prompts", () => {
  const record = { agent_type: "arbitrary-provider", boot_stage: "agent_ready", pty_id: "owned-pty", session_binding_nonce: "nonce" };
  assert.equal(probeReady(record, firstTurnPolicy(null)), false);
  assert.equal(probeReady(record, { lazy: true, structuralComposer: false }), false);
  assert.equal(probeReady({ ...record, pending_prompt: {} }, { lazy: true, structuralComposer: true }), false);
  assert.equal(probeReady({ ...record, boot_stage: "agent_launching" }, { lazy: true, structuralComposer: true }), false);
});

test("declared prefix matching keeps the own-turn boundary and tolerates an output suffix", () => {
  const suffixed = { type: "assistant", content: `${marker}<|eos|>` };
  assert.equal(exactReply([user, suffixed], 0, prompt, marker), null);
  assert.equal(exactReply([user, suffixed], 0, prompt, marker, true), suffixed.content);
  assert.equal(exactReply([suffixed], 0, prompt, marker, true), null);
});
