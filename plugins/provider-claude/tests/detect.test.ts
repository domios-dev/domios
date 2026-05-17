/**
 * Detection tests for Claude provider.
 * Run: npx tsx plugins/provider-claude/tests/detect.test.ts
 */

import { detect, titlePattern, noisePatterns } from "../src/detect";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`); }
}

console.log("\n--- titlePattern ---\n");

assert(titlePattern.test("claude"), "matches bare 'claude'");
assert(titlePattern.test("Claude Code"), "matches 'Claude Code' (case-insensitive)");
assert(titlePattern.test("✱ Working on task"), "matches Braille spinner glyph ✱");
assert(titlePattern.test("⠋ Generating"), "matches Braille dot spinner ⠋");
assert(!titlePattern.test("bash"), "rejects 'bash'");
assert(!titlePattern.test("powershell"), "rejects 'powershell'");
assert(!titlePattern.test("zsh"), "rejects 'zsh'");
assert(!titlePattern.test(""), "rejects empty string");

console.log("\n--- detect(sample) — via title ---\n");

assert(detect({ bytes: "", title: "claude" }), "title 'claude' positive");
assert(detect({ bytes: "", title: "Claude Code" }), "title 'Claude Code' positive");
assert(!detect({ bytes: "", title: "bash" }), "title 'bash' negative");

console.log("\n--- detect(sample) — via PTY bytes ---\n");

assert(
  detect({ bytes: "Some output\n? for shortcuts\n" }),
  "PTY fingerprint '? for shortcuts' positive"
);
assert(
  detect({ bytes: "esc to interrupt" }),
  "PTY fingerprint 'esc to interrupt' positive"
);
assert(
  detect({ bytes: "✱ Brewing..." }),
  "PTY fingerprint '✱ ' positive"
);
assert(
  detect({ bytes: "✻ Thinking..." }),
  "PTY fingerprint '✻ ' positive"
);
assert(
  !detect({ bytes: "$ ls\nfile.txt\n" }),
  "plain shell output negative"
);
assert(
  !detect({ bytes: "PS C:\\> Get-ChildItem" }),
  "PowerShell prompt negative"
);

console.log("\n--- noisePatterns — TUI noise filter ---\n");

const noiseTexts = [
  "shift+tab to cycle modes",
  "Update available! Run brew upgrade",
  "esc to interrupt",
  "? for shortcuts",
  "ctrl+g to edit",
  "Monthly spend limit",
  "hold Space to record",
  "👍",
  "✱ Brewing…",
];

for (const text of noiseTexts) {
  const matched = noisePatterns.some(rx => rx.test(text));
  assert(matched, `noise match: "${text}"`);
}

const cleanTexts = [
  "hello world",
  "function main() {",
  "TODO: implement this",
  "claude is great",  // not noise even though contains 'claude'
];

for (const text of cleanTexts) {
  const matched = noisePatterns.some(rx => rx.test(text));
  assert(!matched, `not noise: "${text}"`);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
