/**
 * Plugin contract tests for Claude — meta, statusOf, actions, detect.
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
