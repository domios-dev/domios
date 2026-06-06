/**
 * Plugin contract tests for Claude — parseLine adapter, meta, actions.
 * Run: npx tsx plugins/provider-claude/tests/plugin.test.ts
 */

import plugin from "../src/index";

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

console.log("\n--- meta ---\n");

eq(plugin.meta.id, "claude", "meta.id is 'claude'");
eq(plugin.meta.displayName, "Claude Code", "meta.displayName");

console.log("\n--- parseLine adapter ---\n");

// Empty input
eq(plugin.parseLine(""), [], "empty line → no events");
eq(plugin.parseLine("   "), [], "whitespace-only line → no events");

// User message
{
  const line = '{"type":"user","uuid":"u1","timestamp":"2025-01-01T00:00:00Z","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}';
  const events = plugin.parseLine(line);
  eq(events.length, 1, "user JSONL → one event");
  eq(events[0].kind, "user_message", "user → kind:user_message");
  assert(events[0].payload !== undefined, "event has payload (ChatMessage)");
  assert(typeof events[0].ts === "number", "event has numeric ts");
}

// Assistant text + tool_call from a single line
{
  const line = '{"type":"assistant","uuid":"a1","timestamp":"2025-01-01T00:00:02Z","message":{"role":"assistant","content":[{"type":"text","text":"Sure"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}';
  const events = plugin.parseLine(line);
  eq(events.length, 2, "assistant with text+tool → two events");
  eq(events[0].kind, "assistant_text", "first event kind");
  eq(events[1].kind, "tool_call", "second event kind");
}

// Malformed JSON → no crash, no events
{
  const events = plugin.parseLine("not valid json");
  eq(events, [], "malformed line → no events, no throw");
}

// Unknown ChatMessage.type (forward-compat through mapping)
{
  // We can't easily synth a parser output with a new type — instead
  // exercise the chatMessageTypeToEventKind by pushing through a
  // known shape. Skipping this — covered by existing parser tests.
}

console.log("\n--- statusOf delegation ---\n");

eq(
  plugin.statusOf({ kind: "user_message" }),
  { kind: "agent_working" },
  "plugin.statusOf delegates to status.ts"
);

console.log("\n--- actions ---\n");

assert(plugin.actions.length >= 2, "has at least 2 actions");

const compact = plugin.actions.find(a => a.id === "compact");
assert(compact !== undefined, "has 'compact' action");
eq(compact?.label, "/compact", "compact action label");
eq(compact?.run({ events: [] }), "/compact\n", "compact action returns slash-command with newline");

const exit = plugin.actions.find(a => a.id === "exit");
assert(exit !== undefined, "has 'exit' action");
eq(exit?.run({ events: [] }), "/exit\n", "exit action returns slash-command with newline");

console.log("\n--- detect delegation ---\n");

assert(plugin.detect({ bytes: "", title: "claude" }), "plugin.detect delegates to detect.ts");

console.log("\n--- imageSend strategy ---\n");

eq(plugin.imageSend?.method, "clipboard-paste", "imageSend method is clipboard-paste");
eq(plugin.imageSend?.pasteKeystroke, "\x16", "imageSend pasteKeystroke is Ctrl+V (0x16)");

console.log("\n--- titlePattern + noisePatterns exposed ---\n");

assert(plugin.titlePattern instanceof RegExp, "titlePattern is a RegExp");
assert(Array.isArray(plugin.noisePatterns) && plugin.noisePatterns.length > 0, "noisePatterns is non-empty array");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
