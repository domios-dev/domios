/**
 * Status mapping tests for Claude provider.
 * Run: npx tsx plugins/provider-claude/tests/status.test.ts
 */

import { statusOf } from "../src/status";
import type { ProviderEvent } from "../../../src/lib/plugins/types";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`); }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

const event = (kind: string, payload?: unknown): ProviderEvent => ({ kind, payload });

console.log("\n--- statusOf ---\n");

eq(
  statusOf(event("user_message")),
  { kind: "agent_working" },
  "user_message → agent_working"
);

eq(
  statusOf(event("assistant_message")),
  { kind: "agent_done" },
  "assistant_message → agent_done"
);

eq(
  statusOf(event("assistant_text")),
  { kind: "agent_done" },
  "assistant_text → agent_done"
);

eq(
  statusOf(event("permission_prompt", { question: "Allow read?" })),
  { kind: "agent_waiting_input", question: "Allow read?" },
  "permission_prompt → agent_waiting_input with question"
);

eq(
  statusOf(event("permission_prompt")),
  { kind: "agent_waiting_input", question: undefined },
  "permission_prompt without payload → agent_waiting_input, question undefined"
);

eq(
  statusOf(event("tool_call")),
  null,
  "tool_call → null (no status change)"
);

eq(
  statusOf(event("tool_result")),
  null,
  "tool_result → null"
);

eq(
  statusOf(event("thinking")),
  null,
  "thinking → null"
);

eq(
  statusOf(event("error", { message: "Connection lost" })),
  { kind: "agent_error", message: "Connection lost" },
  "error → agent_error with message"
);

eq(
  statusOf(event("error")),
  { kind: "agent_error", message: undefined },
  "error without payload → agent_error, message undefined"
);

eq(
  statusOf(event("unknown_future_event")),
  null,
  "unknown event kind → null (forward compat)"
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
