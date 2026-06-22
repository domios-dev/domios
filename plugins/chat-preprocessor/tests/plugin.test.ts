/**
 * Chat-preprocessor plugin contract tests.
 * Run: npx tsx plugins/chat-preprocessor/tests/plugin.test.ts
 */

import plugin, { preprocessText } from "../src/plugin";

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

console.log("\n--- meta ---\n");
eq(plugin.meta.id, "chat-preprocessor", "meta.id is stable");
eq(plugin.match, "text", "declares text slot");

console.log("\n--- disabled/pass-through ---\n");
eq(preprocessText("hello", { enabled: false, activePreset: "mem-recall" }), null, "disabled returns passthrough null");
eq(preprocessText("hello", { enabled: true, activePreset: "missing" }), null, "missing user preset returns passthrough null");

console.log("\n--- template presets ---\n");
eq(
  preprocessText("hello", {
    enabled: true,
    activePreset: "wrap",
    presets: [{ id: "wrap", kind: "template", template: "prefix {{text}} suffix" }],
  }),
  "prefix hello suffix",
  "template preset renders text token",
);

console.log("\n--- mem recall via host ---\n");

(globalThis as unknown as { host: unknown }).host = {
  settingsJson: () => JSON.stringify({
    enabled: true,
    activePreset: "mem-recall",
    memK: 2,
    memMaxChars: 24,
    insertPosition: "prepend",
  }),
  mem: {
    search: ({ query, k }: { query: string; k?: number }) => ({
      hits: [
        { id: "my_mem_1", text: `fact for ${query}`, score: 0.9 },
        { id: "my_mem_2", text: "second fact with a very long body that should be shortened", score: 0.8 },
        { id: "my_mem_3", text: "not included", score: 0.7 },
      ].slice(0, k),
    }),
  },
};

const out = plugin.chatPreprocess({ text: "hello" });
assert(out && typeof out === "object" && "text" in out, "chatPreprocess returns replacement when mem hits exist");
const text = (out as { text: string }).text;
assert(text.startsWith("Found similar in mem:\n- my_mem_1:"), "mem block prepended");
assert(text.includes("- my_mem_2:"), "second mem hit included");
assert(!text.includes("my_mem_3"), "respects memK");
assert(text.endsWith("\n\nhello"), "original text preserved after mem block");

console.log("\n--- mem errors/empty ---\n");
(globalThis as unknown as { host: unknown }).host = {
  settingsJson: () => JSON.stringify({ enabled: true, activePreset: "mem-recall" }),
  mem: { search: () => ({ error: "down" }) },
};
eq(plugin.chatPreprocess({ text: "hello" }), null, "empty/error mem search passes through");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
