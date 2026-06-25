import { formatTaskFreshness, taskFreshnessAt } from "./taskFreshness";
import { FIXTURE_TASK } from "./fixtures.testdata";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const now = 1_700_000_000_000;

assert(formatTaskFreshness(0, now) === "", "zero stamp -> empty");
assert(formatTaskFreshness(now - 5_000, now) === "now", `5s -> now, got ${formatTaskFreshness(now - 5_000, now)}`);
assert(formatTaskFreshness(now - 5 * 60_000, now) === "5m", `5m, got ${formatTaskFreshness(now - 5 * 60_000, now)}`);
assert(formatTaskFreshness(now - 3 * 3_600_000, now) === "3h", `3h, got ${formatTaskFreshness(now - 3 * 3_600_000, now)}`);
assert(formatTaskFreshness(now - 2 * 86_400_000, now) === "2d", `2d, got ${formatTaskFreshness(now - 2 * 86_400_000, now)}`);

assert(taskFreshnessAt(FIXTURE_TASK) == null, "fixture without stamps -> null");
assert(
  taskFreshnessAt({ ...FIXTURE_TASK, status_changed_at: 100, updated_at: 200 }) === 100,
  "status_changed_at wins over updated_at",
);
assert(
  taskFreshnessAt({ ...FIXTURE_TASK, updated_at: 200, created_at: 100 }) === 200,
  "updated_at wins over created_at",
);
assert(taskFreshnessAt({ ...FIXTURE_TASK, created_at: 100 }) === 100, "created_at fallback");

console.log("taskFreshness: all passed");
