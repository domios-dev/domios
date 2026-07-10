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
  assert(isTransientFetchError(new Error("plans list failed: 503 plans are still loading")), "503 loading is retryable");
  assert(!isTransientFetchError(new DOMException("Aborted", "AbortError")), "AbortError is not transient");
});

test("formatPlanFetchError hides raw transport strings", () => {
  const msg = formatPlanFetchError(new TypeError("Load failed"));
  assert(!msg.includes("TypeError"), "no raw exception name");
  assert(!msg.includes("Load failed"), "no raw browser message");
  assert(msg.includes("Reconnecting"), "friendly reconnect copy");
});

test("non-gateway 5xx and 4xx are not transient (surface real error, no silent retry-only)", () => {
  assert(!isTransientFetchError(new Error("plans list failed: 500 internal")), "500 is a real error");
  assert(!isTransientFetchError(new Error("plans list failed: 404 not found")), "404 is a real error");
  assert(!isTransientFetchError(new Error("plans list failed: 401 unauthorized")), "401 is a real error");
});

test("formatPlanFetchError passes through status-prefixed messages verbatim", () => {
  assert(formatPlanFetchError(new Error("500 internal error")).includes("500"), "keeps 500 code");
  const generic = formatPlanFetchError(new Error("boom"));
  assert(generic.includes("Try again"), "unknown non-transient error gets generic copy");
});

test("empty/unknown error is treated as transient (reconnect, not hard error)", () => {
  assert(isTransientFetchError(new Error("")), "empty message reconnects rather than hard-fails");
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
