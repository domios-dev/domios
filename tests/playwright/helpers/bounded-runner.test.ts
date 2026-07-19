import assert from "node:assert/strict";
import test from "node:test";
import {
  boundedPlaywrightArgs,
  budgetsForWorkers,
  parseBoundedPlaywrightArgs,
  runWithGuaranteedCleanup,
} from "../../../scripts/run-playwright-bounded";
import { devInstanceForSlot, type InstanceLifecycle } from "./dev-instance";

const API_SPEC = "tests/playwright/suites/11-api/api-core.spec.ts";
const CANVAS_SPEC = "tests/playwright/suites/13-canvas/quiet-tab-chrome.spec.ts";

function twoInstances() {
  return [devInstanceForSlot(0, 2), devInstanceForSlot(1, 2)];
}

function recordingLifecycle(failOn?: string): { lifecycle: InstanceLifecycle; killed: string[]; stopped: string[] } {
  const killed: string[] = [];
  const stopped: string[] = [];
  return {
    killed,
    stopped,
    lifecycle: {
      kill: (instance) => killed.push(instance.name),
      assertStopped: async (instance) => {
        stopped.push(instance.name);
        if (instance.name === failOn) throw new Error(`leaked ${instance.name}`);
      },
    },
  };
}

test("bounded runner accepts multiple specs and reserves teardown inside 600 seconds", () => {
  const parsed = parseBoundedPlaywrightArgs([
    "--file", API_SPEC,
    "--file", CANVAS_SPEC,
    "--project", "golden",
    "--grep", "focused acceptance",
    "--workers", "2",
  ]);
  const budgets = budgetsForWorkers(2);
  assert.equal(budgets.totalBudgetMs, 600_000);
  assert.equal(budgets.childDeadlineMs, 410_000);
  assert.equal(budgets.innerGlobalTimeoutMs, 395_000);
  const childArgs = boundedPlaywrightArgs(parsed);
  assert.ok(childArgs.includes(`--global-timeout=${budgets.innerGlobalTimeoutMs}`));
  assert.ok(childArgs.includes("--workers=2"));
  assert.ok(childArgs.includes(API_SPEC));
  assert.ok(childArgs.includes(CANVAS_SPEC));
  assert.ok(childArgs.includes("--reporter=line,json"));
});

test("bounded runner rejects broad or unbounded invocation shapes", () => {
  assert.throws(
    () => parseBoundedPlaywrightArgs(["--project", "golden", "--workers", "2"]),
    /--file must be supplied/,
  );
  assert.throws(
    () => parseBoundedPlaywrightArgs(["--file", API_SPEC, "--project", "golden", "--workers", "many"]),
    /positive integer/,
  );
  assert.throws(
    () => parseBoundedPlaywrightArgs(["--file", "package.json", "--project", "golden", "--workers", "2"]),
    /under tests\/playwright/,
  );
});

test("nonzero fake child still tears down both worker instances", async () => {
  const { lifecycle, killed, stopped } = recordingLifecycle();
  const failure = new Error("fake child exited with code 1");
  await assert.rejects(
    () => runWithGuaranteedCleanup(async () => { throw failure; }, twoInstances(), lifecycle),
    failure,
  );
  assert.deepEqual(killed, ["pw2", "pw1"]);
  assert.deepEqual(stopped, ["pw2", "pw1"]);
});

test("deadline fake child and cleanup leak surface together after sweeping both slots", async () => {
  const { lifecycle, killed, stopped } = recordingLifecycle("pw2");
  const { childDeadlineMs } = budgetsForWorkers(2);
  await assert.rejects(
    () => runWithGuaranteedCleanup(
      async () => { throw new Error(`bounded Playwright run exceeded ${childDeadlineMs}ms`); },
      twoInstances(),
      lifecycle,
    ),
    (error: unknown) => {
      assert.ok(error instanceof AggregateError);
      assert.equal(error.errors.length, 2);
      assert.match(String(error.errors[0]), /exceeded/);
      assert.match(String(error.errors[1]), /leaked pw2/);
      return true;
    },
  );
  assert.deepEqual(killed, ["pw2", "pw1"]);
  assert.deepEqual(stopped, ["pw2", "pw1"]);
});
