/**
 * WHAT: the helpers/ timer rules catch the harness's own sleeps.
 * WHY: check-tags walked ../suites only, so helpers/ was unpoliced — which is
 *      exactly where the one unconditional sleep-in-disguise lived.
 * ORACLE: each defect shape produces a violation; each proof shape produces none.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { REPO_ROOT } from "./dev-console";
import { helperSourceFiles, scanTimerViolations, TIMER_RULE_REASONS } from "./timer-rules.mjs";

const HELPERS_DIR = path.join(REPO_ROOT, "tests", "playwright", "helpers");

function scan(source: string): string[] {
  return scanTimerViolations(source, "helpers/subject.ts");
}

test("the service-worker race that always expired is a violation", () => {
  const violations = scan(`
async function waitForShellOnce(page) {
  await page.evaluate(() =>
    "serviceWorker" in navigator
      ? Promise.race([
          navigator.serviceWorker.ready.then(() => undefined),
          new Promise((resolve) => setTimeout(resolve, 1_000)),
        ])
      : undefined,
  );
}
`);
  assert.equal(violations.length, 1, `expected exactly 1 violation, got ${violations.length}`);
  assert.match(violations[0], /^helpers\/subject\.ts:7 /);
  assert.equal(violations[0].endsWith(TIMER_RULE_REASONS.raceResolvedByTimer), true);
});

test("a race whose timer only rejects is a proof, not a violation", () => {
  assert.deepEqual(
    scan(`
async function ready(page) {
  await page.evaluate(() =>
    new Promise((resolve, reject) => {
      const failure = setTimeout(() => reject(new Error("deadline")), 30_000);
      navigator.serviceWorker.ready.then(() => { clearTimeout(failure); resolve(); }, reject);
    }),
  );
}
`),
    [],
  );
});

test("a fixed delay awaited outside a loop is a violation", () => {
  const violations = scan(`
async function warmup() {
  await new Promise((resolve) => setTimeout(resolve, 500));
}
`);
  assert.equal(violations.length, 1, `expected exactly 1 violation, got ${violations.length}`);
  assert.equal(violations[0], `helpers/subject.ts:3 ${TIMER_RULE_REASONS.fixedDelayAwait}`);
});

test("the same delay inside a bounded poll loop is an interval, not a violation", () => {
  assert.deepEqual(
    scan(`
async function pollUntil(fn, deadline) {
  while (Date.now() < deadline) {
    if (await fn()) return true;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  return false;
}
`),
    [],
  );
});

test("an indirect resolve inside the timer callback is still a delay", () => {
  const violations = scan(`
async function warmup() {
  await new Promise((r) => setTimeout(() => r(), 750));
}
`);
  assert.equal(violations.length, 1, `expected exactly 1 violation, got ${violations.length}`);
  assert.equal(violations[0], `helpers/subject.ts:3 ${TIMER_RULE_REASONS.fixedDelayAwait}`);
});

test("waitForTimeout is banned in helpers as it already is in suites", () => {
  const violations = scan(`
async function drive(page) {
  await page.waitForTimeout(16);
}
`);
  assert.equal(violations.length, 1, `expected exactly 1 violation, got ${violations.length}`);
  assert.equal(violations[0], `helpers/subject.ts:3 ${TIMER_RULE_REASONS.waitForTimeout}`);
});

test("an annotated stimulus is exempt in helpers on the same PW_JUSTIFIED_SLEEP terms", () => {
  assert.deepEqual(
    scan(`
async function delayRoute(page, delayMs) {
  // PW_JUSTIFIED_SLEEP: stimulus — the injected latency IS the test input.
  await new Promise((resolve) => setTimeout(resolve, delayMs));
}
`),
    [],
  );
});

test("an unannotated timer next to an annotated one is still reported", () => {
  const violations = scan(`
async function two(page) {
  // PW_JUSTIFIED_SLEEP: stimulus — injected latency.
  await new Promise((resolve) => setTimeout(resolve, 10));
  await new Promise((resolve) => setTimeout(resolve, 20));
}
`);
  assert.equal(violations.length, 1, `expected exactly 1 violation, got ${violations.length}`);
  assert.equal(violations[0], `helpers/subject.ts:5 ${TIMER_RULE_REASONS.fixedDelayAwait}`);
});

test("the walk covers helpers/ — the blind spot — and excludes its own tests", () => {
  const files = helperSourceFiles(HELPERS_DIR).map((file: string) => path.basename(file));
  assert.equal(files.includes("dev-console.ts"), true, "dev-console.ts must be policed");
  assert.equal(files.includes("fixtures.ts"), true, "fixtures.ts must be policed");
  assert.equal(
    files.some((name: string) => name.endsWith(".test.ts")),
    false,
    "helper unit tests carry rule fixtures and are not harness runtime",
  );
});

test("every helper the harness runs is currently free of unannotated timers", () => {
  const offenders = helperSourceFiles(HELPERS_DIR).flatMap((file: string) =>
    scanTimerViolations(readFileSync(file, "utf8"), path.basename(file)),
  );
  assert.deepEqual(offenders, [], offenders.join("\n"));
});

test("reintroducing the removed race into a real helper is caught by the same walk", () => {
  const devConsole = readFileSync(path.join(HELPERS_DIR, "dev-console.ts"), "utf8");
  assert.deepEqual(scanTimerViolations(devConsole, "dev-console.ts"), []);
  const regressed = devConsole.replace(
    "await waitForServiceWorkerControl(page);",
    `await page.evaluate(() =>
    Promise.race([
      navigator.serviceWorker.ready.then(() => undefined),
      new Promise<void>((resolve) => setTimeout(resolve, 1_000)),
    ]),
  );`,
  );
  assert.notEqual(regressed, devConsole, "the anchor must still exist in dev-console.ts");
  const violations = scanTimerViolations(regressed, "dev-console.ts");
  assert.equal(violations.length, 1, `expected exactly 1 violation, got ${violations.length}`);
  assert.equal(violations[0].endsWith(TIMER_RULE_REASONS.raceResolvedByTimer), true);
});

test("a deadline timer held in a variable is not a delay promise", () => {
  assert.deepEqual(
    scan(`
function run(child, timeoutMs) {
  const deadline = setTimeout(() => child.kill(), timeoutMs);
  return deadline;
}
`),
    [],
  );
});
