// Run: npx tsx src/components/Canvas/planView/textTruncate.test.ts

import { middleTruncateSlug } from "./textTruncate";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("short path unchanged", () => {
  assert(middleTruncateSlug("lane/x") === "lane/x", "unchanged");
});

test("long slug keeps tail after slash", () => {
  const out = middleTruncateSlug("lane/task-73ea1b9d-card-metadata", 24);
  assert(out.includes("…"), "ellipsis");
  assert(out.endsWith("card-metadata"), "tail slug");
  assert(out.startsWith("lane/"), "prefix kept");
});

let passed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    passed++;
    console.log(`  ✓ ${name}`);
  } catch (e) {
    console.error(`  ✗ ${name}`);
    console.error(e);
    process.exit(1);
  }
}
console.log(`\n${passed}/${tests.length} passed`);
