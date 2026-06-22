// Run: npx tsx src/lib/formatModelBadge.test.ts

import { formatModelBadge, MODEL_BADGE_MAX_CHARS } from "./formatModelBadge";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}

function assertEqual(actual: string, expected: string, message: string) {
  if (actual !== expected) throw new Error(`${message}: expected "${expected}", got "${actual}"`);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("strips a leading vendor prefix", () => {
  assertEqual(
    formatModelBadge("NVIDIA/NEMOTRON-3-NANO"),
    "NEMOTRON-3-NANO",
    "vendor prefix should be removed",
  );
});

test("drops stacked quantization suffixes when the id is over cap", () => {
  assertEqual(
    formatModelBadge("GOOGLE/GEMMA-4-26B-A4B-QAT"),
    "GEMMA-4-26B",
    "quantization suffixes should be stripped",
  );
});

test("leaves short model names unchanged", () => {
  assertEqual(formatModelBadge("OPUS"), "OPUS", "OPUS should pass through");
  assertEqual(formatModelBadge("Sonnet"), "Sonnet", "Sonnet should pass through");
});

test("ellipsizes very long ids within the cap", () => {
  const badge = formatModelBadge("ACME/THIS-MODEL-FAMILY-NAME-IS-TOO-LONG-120B");
  assert(badge.length <= MODEL_BADGE_MAX_CHARS, `badge should fit cap, got ${badge.length}`);
  assert(badge.endsWith("..."), `badge should end-ellipsize, got ${badge}`);
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (error) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(error);
  }
}
console.log(`formatModelBadge: ${tests.length - failed}/${tests.length} passed`);
if (failed > 0) process.exit(1);
