// Run: npx tsx src/lib/formatModelBadge.test.ts

import { formatModelBadge, modelBadgeWidthForProvider, MODEL_BADGE_MAX_CHARS } from "./formatModelBadge";

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

test("caps the badge to roughly the provider-label width", () => {
  assertEqual(String(modelBadgeWidthForProvider("opencode")), "8", "opencode label is eight wide");
  assertEqual(String(modelBadgeWidthForProvider("codex")), "8", "a short provider keeps the floor");
  const badge = formatModelBadge("deepseek-v4-flash", modelBadgeWidthForProvider("opencode"));
  assert(badge.length <= 8, `badge should fit the provider width, got ${badge.length}`);
  assert(badge.endsWith("..."), `badge should end-ellipsize, got ${badge}`);
});

test("never exceeds the shared maximum for a very long provider label", () => {
  const width = modelBadgeWidthForProvider("a-very-long-provider-label");
  assert(width === MODEL_BADGE_MAX_CHARS, `provider width should clamp to the cap, got ${width}`);
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
