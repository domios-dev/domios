import assert from "node:assert/strict";
import test from "node:test";
import { readableSystemReminder } from "./systemReminderText";

test("text without internal markers stays untouched", () => {
  assert.equal(readableSystemReminder("plain-body"), null);
});

test("worker charter marker becomes a titled readable section", () => {
  const readable = readableSystemReminder('<codeterm-worker-charter task="task_q1">\nfrag-a\nfrag-b\n</codeterm-worker-charter>');
  assert.ok(readable);
  assert.equal(readable.text, "frag-a\nfrag-b");
  assert.equal(readable.sections.length, 1);
  assert.equal(readable.sections[0].kind, "worker-charter");
  assert.ok(readable.title.includes("task_q1"));
  assert.ok(!readable.title.includes("<"));
});

test("orchestrator heartbeat keeps surrounding text and drops stray markers", () => {
  const source = 'lead-frag\n<codeterm-orchestrator-heartbeat plan="pl_x">\nstate-frag\n</codeterm-orchestrator-heartbeat>\ntail-frag </codeterm-role-identity>';
  const readable = readableSystemReminder(source);
  assert.ok(readable);
  assert.deepEqual(readable.sections.map((section) => section.body), ["lead-frag", "state-frag", "tail-frag"]);
  assert.ok(readable.title.includes("pl_x"));
  assert.ok(!readable.text.includes("<codeterm-") && !readable.text.includes("</codeterm-"));
});

test("unknown and unclosed markers never leak raw tags", () => {
  const unknown = readableSystemReminder('<codeterm-future-thing id="z">\nbody-frag\n</codeterm-future-thing>');
  assert.ok(unknown);
  assert.equal(unknown.text, "body-frag");
  assert.ok(!unknown.title.includes("codeterm-"));
  const unclosed = readableSystemReminder('<codeterm-worker-charter task="t">\ncut-frag');
  assert.ok(unclosed);
  assert.equal(unclosed.text, "cut-frag");
});

test("orphan closing markers are recognized and stripped", () => {
  const readable = readableSystemReminder("orphan-frag\n</codeterm-worker-charter>");
  assert.ok(readable);
  assert.equal(readable.text, "orphan-frag");
  assert.equal(typeof readable.title, "string");
});

test("prose mentioning the prefix without a marker token is not a reminder", () => {
  assert.equal(readableSystemReminder("literal <codeterm- text"), null);
  assert.equal(readableSystemReminder("see <codeterm-x attr=unquoted> here"), null);
});

test("marker names that shadow Object prototype members get readable string titles", () => {
  for (const name of ["constructor"]) {
    const readable = readableSystemReminder(`<codeterm-${name}>\nproto-frag\n</codeterm-${name}>`);
    assert.ok(readable);
    assert.equal(typeof readable.title, "string");
    assert.ok(!readable.title.includes("codeterm-"));
    assert.equal(readable.text, "proto-frag");
  }
});
