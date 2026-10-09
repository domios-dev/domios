import assert from "node:assert/strict";
import test from "node:test";
import { exactReply } from "./oracle.mjs";

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
