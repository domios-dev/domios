import assert from "node:assert/strict";
import { test } from "node:test";
import { forkableTurnEnds } from "./messageForkModel";

const rows = [
  { id: "question", toolFlowId: "one" },
  { id: "answer", toolFlowId: "one" },
  { id: "later-question", toolFlowId: "two" },
  { id: "later-answer", toolFlowId: "two" },
];

test("turn forks offer only inclusive turn ends, including older completed turns", () => {
  assert.deepEqual([...forkableTurnEnds(rows, true)], ["later-answer", "answer"]);
});

test("whole-session providers never offer a message cut they cannot execute", () => {
  assert.deepEqual([...forkableTurnEnds(rows, false)], []);
});

test("a historical window cannot mistake its truncated tail for the end of a turn", () => {
  assert.deepEqual([...forkableTurnEnds(rows.slice(0, 1), true, true)], []);
});

test("unbound rows do not gain fork actions and interleaved rows keep the actual turn end", () => {
  assert.deepEqual([...forkableTurnEnds([
    rows[0], rows[2], { id: "notice" }, rows[1], rows[3],
  ], true)], ["later-answer", "answer"]);
  assert.deepEqual([...forkableTurnEnds([], true)], []);
});
