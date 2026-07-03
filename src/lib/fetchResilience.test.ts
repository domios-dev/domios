// Run: npx tsx src/lib/fetchResilience.test.ts

import {
  computeBackoffDelay,
  formatPlanFetchError,
  isTransientFetchError,
} from "./fetchResilience";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("isTransientFetchError flags WKWebView load failures", () => {
  assert(isTransientFetchError(new TypeError("Load failed")), "TypeError: Load failed");
  assert(isTransientFetchError(new TypeError("Failed to fetch")), "Failed to fetch");
  assert(!isTransientFetchError(new DOMException("Aborted", "AbortError")), "AbortError is not transient");
});

test("formatPlanFetchError hides raw transport strings", () => {
  const msg = formatPlanFetchError(new TypeError("Load failed"));
  assert(!msg.includes("TypeError"), "no raw exception name");
  assert(!msg.includes("Load failed"), "no raw browser message");
  assert(msg.includes("Reconnecting"), "friendly reconnect copy");
});

test("computeBackoffDelay grows with attempt and stays bounded", () => {
  const d1 = computeBackoffDelay(1, 1_000, 15_000);
  const d3 = computeBackoffDelay(3, 1_000, 15_000);
  const d8 = computeBackoffDelay(8, 1_000, 15_000);
  assert(d1 >= 500 && d1 <= 1_000, `attempt 1 in [500,1000] got ${d1}`);
  assert(d3 > d1, "attempt 3 delay exceeds attempt 1");
  assert(d8 <= 15_000, `attempt 8 capped at 15s got ${d8}`);
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
