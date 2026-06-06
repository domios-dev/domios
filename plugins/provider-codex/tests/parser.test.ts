/**
 * Codex rollout JSONL parser tests.
 * Run: pnpm dlx tsx plugins/provider-codex/tests/parser.test.ts
 */

import { parseJsonlSession } from "../src/parser";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`);
  }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

console.log("\n--- parseJsonlSession ---\n");

const input = [
  JSON.stringify({
    type: "response_item",
    timestamp: "2026-05-23T10:00:00.000Z",
    payload: {
      type: "message",
      id: "u1",
      role: "user",
      content: [{ type: "input_text", text: "make a file" }],
    },
  }),
  JSON.stringify({
    type: "response_item",
    timestamp: "2026-05-23T10:00:01.000Z",
    payload: {
      type: "message",
      id: "a1",
      role: "assistant",
      content: [{ type: "output_text", text: "Created test.txt." }],
    },
  }),
  JSON.stringify({
    type: "response_item",
    payload: {
      type: "custom_tool_call",
      call_id: "call-1",
      name: "shell",
      input: { command: "type test.txt" },
    },
  }),
  JSON.stringify({
    type: "response_item",
    payload: {
      type: "custom_tool_call_output",
      call_id: "call-1",
      output: "test",
    },
  }),
].join("\n");

const messages = parseJsonlSession(input);

eq(messages.map((m) => m.type), ["user", "assistant", "tool_call", "tool_result"], "message types");
eq(messages[0].content, "make a file", "user text");
eq(messages[1].content, "Created test.txt.", "assistant text");
eq(messages[2].toolName, "shell", "tool name");
eq(messages[3].content, "test", "tool output");
assert(messages.every((m) => m.provider === "codex"), "provider marker");

const contextual = parseJsonlSession(JSON.stringify({
  type: "response_item",
  timestamp: "2026-05-25T10:00:00.000Z",
  payload: {
    type: "message",
    id: "u2",
    role: "user",
    content: [{
      type: "input_text",
      text: "<environment_context>\n  <current_date>2026-05-25</current_date>\n  <timezone>Europe/Riga</timezone>\n</environment_context>\n\nда просто интересно как работает это вообще?",
    }],
  },
}));

eq(contextual.map((m) => m.type), ["system", "user"], "environment_context split from user turn");
eq(contextual[0].tagName, "environment_context", "environment_context tag");
assert(contextual[0].content.includes("<current_date>2026-05-25</current_date>"), "environment_context content kept");
eq(contextual[1].content, "да просто интересно как работает это вообще?", "actual user text preserved");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
