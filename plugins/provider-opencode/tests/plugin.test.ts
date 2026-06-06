/**
 * Opencode plugin contract + detection tests.
 * Run: npx tsx plugins/provider-opencode/tests/plugin.test.ts
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
eq(plugin.meta.id, "opencode", "meta.id is 'opencode'");
eq(plugin.meta.displayName, "OpenCode", "meta.displayName");

console.log("\n--- detect ---\n");

assert(plugin.detect({ bytes: "", title: "opencode" }), "title 'opencode' positive");
assert(plugin.detect({ bytes: "", title: "OPENCODE-x" }), "title uppercase positive (case-insensitive)");
assert(plugin.detect({ bytes: "Running opencode v1.2" }), "PTY 'opencode' substring positive");
assert(plugin.detect({ bytes: "OPENCODE banner" }), "PTY 'OPENCODE' substring positive");
assert(!plugin.detect({ bytes: "$ ls\n", title: "bash" }), "plain shell negative");
assert(!plugin.detect({ bytes: "", title: "codex" }), "codex title is not opencode");

console.log("\n--- parseLine ---\n");
eq(plugin.parseLine('{"x":1}'), [], "parseLine returns empty (no JSONL support)");
eq(plugin.parseLine(""), [], "empty line → empty events");

console.log("\n--- statusOf ---\n");
eq(plugin.statusOf({ kind: "anything" }), null, "statusOf always returns null");

console.log("\n--- actions ---\n");
eq(plugin.actions, [], "no actions defined yet");

console.log("\n--- titlePattern + noisePatterns ---\n");
assert(plugin.titlePattern instanceof RegExp, "titlePattern is RegExp");
assert(plugin.titlePattern!.test("opencode"), "titlePattern matches 'opencode'");
assert(Array.isArray(plugin.noisePatterns) && plugin.noisePatterns.length > 0, "noisePatterns non-empty");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
