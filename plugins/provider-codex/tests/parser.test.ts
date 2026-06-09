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
  JSON.stringify({
    type: "response_item",
    payload: {
      type: "function_call",
      call_id: "call-2",
      name: "shell_command",
      arguments: JSON.stringify({
        command: "Get-Content C:\\Users\\dev\\Downloads\\foo.ps1",
        timeout_ms: 10000,
      }),
    },
  }),
  JSON.stringify({
    type: "response_item",
    payload: {
      type: "function_call_output",
      call_id: "call-2",
      output: "line1\nline2",
    },
  }),
  JSON.stringify({
    type: "response_item",
    payload: {
      type: "function_call",
      call_id: "call-3",
      name: "shell_command",
      arguments: JSON.stringify({
        argv: ["Get-Item", "-Path", "C:\\Users\\dev\\Downloads"],
      }),
    },
  }),
  JSON.stringify({
    type: "response_item",
    payload: {
      type: "function_call_output",
      call_id: "call-3",
      output: "ok",
    },
  }),
].join("\n");

const messages = parseJsonlSession(input);

eq(
  messages.map((m) => m.type),
  ["user", "assistant", "tool_call", "tool_result", "tool_call", "tool_result", "tool_call", "tool_result"],
  "message types",
);
eq(messages[0].content, "make a file", "user text");
eq(messages[1].content, "Created test.txt.", "assistant text");
eq(messages[2].toolName, "Bash", "legacy shell tool name canonicalized");
eq(messages[4].toolName, "Bash", "function_call shell_command canonicalized");
eq(messages[4].toolArgs, "Get-Content C:\\Users\\dev\\Downloads\\foo.ps1", "command extracted from arguments JSON");
eq(messages[6].toolArgs, "Get-Item -Path C:\\Users\\dev\\Downloads", "argv array converted to command");
eq(messages[3].content, "test", "legacy function output");
eq(messages[5].content, "line1\nline2", "function_call output");
eq(messages[7].content, "ok", "function_call argv output");
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

const meta = parseJsonlSession(JSON.stringify({
  type: "response_item",
  timestamp: "2026-05-25T10:00:00.000Z",
  payload: {
    type: "message",
    id: "meta-1",
    role: "user",
    isMeta: true,
    content: [{ type: "input_text", text: "Base directory for this skill: C:\\Users\\dev\\.codex\\skills\\codeterm\n\n# CodeTerm" }],
  },
}));

eq(meta.map((m) => m.type), ["system"], "codex meta user turns render as system");
eq(meta[0].systemSubtype, "skill_context", "codex skill meta subtype");

const codexRuntimeNoise = parseJsonlSession([
  JSON.stringify({
    type: "response_item",
    timestamp: "2026-06-06T18:30:54.000Z",
    payload: {
      type: "message",
      id: "bootstrap",
      role: "user",
      content: [{
        type: "input_text",
        text: "# AGENTS.md instructions for /Users/rollacode/.codeterm-dev/general\n\n<INSTRUCTIONS>\n# AGENTS.md\n\nUse the repo rules.\n</INSTRUCTIONS>",
      }],
    },
  }),
  JSON.stringify({
    type: "response_item",
    timestamp: "2026-06-06T18:31:02.000Z",
    payload: {
      type: "message",
      id: "real-user",
      role: "user",
      content: [{ type: "input_text", text: "Йо?" }],
    },
  }),
  JSON.stringify({
    type: "event_msg",
    timestamp: "2026-06-06T18:31:05.000Z",
    payload: {
      type: "agent_message",
      message: "Йо. Я на месте.",
    },
  }),
  JSON.stringify({
    type: "response_item",
    timestamp: "2026-06-06T18:31:05.000Z",
    payload: {
      type: "message",
      id: "assistant-final",
      role: "assistant",
      content: [{ type: "output_text", text: "Йо. Я на месте." }],
    },
  }),
].join("\n"));

eq(codexRuntimeNoise.map((m) => m.type), ["user", "assistant"], "codex bootstrap and duplicate assistant noise hidden");
eq(codexRuntimeNoise[0].content, "Йо?", "codex real user turn starts chat");
eq(codexRuntimeNoise[1].content, "Йо. Я на месте.", "codex assistant kept once");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
