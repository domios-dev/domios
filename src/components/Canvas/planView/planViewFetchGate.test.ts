// Run: npx tsx src/components/Canvas/planView/planViewFetchGate.test.ts

import { planFetchShowsData, planFetchShowsFullError } from "./planViewFetchGate";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("stale plans keep content visible during error", () => {
  assert(planFetchShowsData(false, "Could not reach CodeTerm", 2), "stale data visible");
  assert(!planFetchShowsFullError(false, "Could not reach CodeTerm", 2), "no full-screen error");
});

test("empty error state blocks content", () => {
  assert(!planFetchShowsData(false, "Could not reach CodeTerm", 0), "no data hidden");
  assert(planFetchShowsFullError(false, "Could not reach CodeTerm", 0), "full error shown");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failed++;
    console.error(`  ✗ ${name}`);
    console.error(`    ${e instanceof Error ? e.message : e}`);
  }
}
console.log(`\n${tests.length - failed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
