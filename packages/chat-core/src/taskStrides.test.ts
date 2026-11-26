import assert from "node:assert/strict";
import test from "node:test";
import { hasTaskStrides, splitTaskStrides, taskStridesLabel } from "./taskStrides";
import { readableSystemReminder } from "./systemReminderText";

test("plain text without injected strides stays untouched", () => {
  assert.equal(hasTaskStrides("plain-body-frag"), false);
  assert.deepEqual(splitTaskStrides("plain-body-frag"), [{ type: "text", text: "plain-body-frag" }]);
});

test("tagged task strides block splits cleanly with count, task id, and surrounding prose", () => {
  const input = [
    "lead-frag-1",
    '<codeterm-task-strides task="task_frag1" count="2">',
    "Recent task strides (read-only digest, pinned/newest first):",
    "- [decision] 2026-11-25 04:00Z frag-dec-a",
    "- [worker_report] 2026-11-25 10:19Z frag-rep-b",
    "Read full strides: `domios plan task show pl_x task_frag1`",
    "</codeterm-task-strides>",
    "tail-frag-2",
  ].join("\n\n");

  assert.equal(hasTaskStrides(input), true);
  const parts = splitTaskStrides(input);
  assert.equal(parts.length, 3);
  assert.deepEqual(parts[0], { type: "text", text: "lead-frag-1" });
  assert.equal(parts[1].type, "strides");
  if (parts[1].type === "strides") {
    assert.equal(parts[1].block.taskId, "task_frag1");
    assert.equal(parts[1].block.count, 2);
    assert.ok(parts[1].block.body.includes("frag-dec-a"));
    assert.ok(parts[1].block.body.includes("domios plan task show pl_x task_frag1"));
    assert.ok(!parts[1].block.body.includes("<codeterm-task-strides"));
  }
  assert.deepEqual(parts[2], { type: "text", text: "tail-frag-2" });
});

test("legacy unwrapped task strides block is also detected and counted", () => {
  const input = [
    "lead-legacy-frag",
    "",
    "Recent task strides (read-only context, pinned/newest first):",
    "- [decision] frag-legacy-1",
    "- [worker_report] frag-legacy-2",
    "- [note] frag-legacy-3",
    "",
    "tail-legacy-frag",
  ].join("\n");

  assert.equal(hasTaskStrides(input), true);
  const parts = splitTaskStrides(input);
  assert.equal(parts.length, 3);
  assert.deepEqual(parts[0], { type: "text", text: "lead-legacy-frag" });
  assert.equal(parts[1].type, "strides");
  if (parts[1].type === "strides") {
    assert.equal(parts[1].block.count, 3);
    assert.ok(parts[1].block.body.includes("frag-legacy-2"));
  }
  assert.deepEqual(parts[2], { type: "text", text: "tail-legacy-frag" });
});

test("taskStridesLabel formats singular and plural labels", () => {
  assert.equal(taskStridesLabel(1), "1 task stride");
  assert.equal(taskStridesLabel(4), "4 task strides");
});

test("readableSystemReminder exposes task-strides metadata without raw marker tags", () => {
  const source = [
    '<codeterm-worker-charter task="task_sr1">',
    "charter-frag-1",
    "</codeterm-worker-charter>",
    "",
    '<codeterm-task-strides task="task_sr1" count="2">',
    "- [decision] 2026-11-25 04:00Z stride-frag-1",
    "- [note] 2026-11-25 05:00Z stride-frag-2",
    "</codeterm-task-strides>",
  ].join("\n");

  const readable = readableSystemReminder(source);
  assert.ok(readable);
  assert.equal(readable.sections.length, 2);
  assert.equal(readable.sections[1].kind, "task-strides");
  assert.equal(readable.sections[1].title, "2 task strides");
  assert.deepEqual(readable.sections[1].taskStrides?.count, 2);
  assert.equal(readable.sections[1].taskStrides?.taskId, "task_sr1");
  assert.ok(!readable.text.includes("<codeterm-"));
});
