import assert from "node:assert/strict";
import test from "node:test";
import { formatDsTasksBlock, parseDsTasksMessage, type DsTasksBlock } from "./dsTasksBlock";
const block: DsTasksBlock = { planId: "pl_alpha", planTitle: "Alpha\nplan", filter: ["in_review", "todo"], items: [{ id: "task_a", key: "AL-1", status: "in_review", name: "First  task" }, { id: "task_b", key: "AL-2", status: "pending", name: "" }] };
test("typed task snapshots preserve exact JSON fields and detached-task fallback", () => {
  const text = formatDsTasksBlock(block);
  assert.ok(!text.includes("Plan:") && !text.includes("filter="));
  const parsed = parseDsTasksMessage(`look at this\n${text}\nthanks`);
  assert.deepEqual(parsed, { before: "look at this", block, after: "thanks" });
  assert.deepEqual(parseDsTasksMessage(formatDsTasksBlock({ ...block, filter: [] }).replace(/\n/g, "\r\n"))?.block, { ...block, filter: [] });
});
test("snapshot parser rejects old wire and literal or malformed snapshots", () => {
  for (const source of ['<ds-tasks plan="pl_alpha">Plan: Alpha\n- AL-1 task_a [pending] A</ds-tasks>', '```\n' + formatDsTasksBlock(block) + '\n```', formatDsTasksBlock(block).replace('"pending"', '3'), '<ds-tasks plan="pl_alpha">{"title":"X","items":[]}', '@ds-tasks pl_alpha']) assert.equal(parseDsTasksMessage(source), null);
});
