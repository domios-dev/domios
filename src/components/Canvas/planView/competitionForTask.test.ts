// Run: npx tsx src/components/Canvas/planView/competitionForTask.test.ts

import { FIXTURE_TASK } from "../cards/fixtures.testdata";
import type { Task } from "../types";
import {
  competitionAttempts,
  competitionForTask,
  isCompetitiveTask,
  liveCompetitionAttemptCount,
} from "./competitionForTask";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("returns null for non-competitive tasks", () => {
  assert(!isCompetitiveTask(FIXTURE_TASK), "fixture not competitive");
  assert(competitionForTask(FIXTURE_TASK) === null, "no badge data");
});

test("uses configured attempt count with a floor of 2", () => {
  const task: Task = {
    ...FIXTURE_TASK,
    competition: { enabled: true, attempts: 4, models: ["gpt-5", "claude"] },
  };
  assert(competitionAttempts(task) === 4, "configured attempts");
  const result = competitionForTask(task);
  assert(result?.count === 4, "badge count");
  assert(result?.tooltip.includes("models: gpt-5, claude"), "models in tooltip");
});

test("live-competition tooltip lists open attempts", () => {
  const task: Task = {
    ...FIXTURE_TASK,
    competition: { enabled: true, attempts: 3 },
    execution_attempts: [
      {
        tab_id: "tab-a",
        tab_title: "Worker A",
        model: "gpt-5",
        competition_group_id: "grp-1",
        competition_lane: 1,
      },
      {
        tab_id: "tab-b",
        tab_title: "Worker B",
        model: "claude",
        competition_group_id: "grp-1",
        competition_lane: 2,
        closed_at: 99,
      },
    ],
  };
  assert(liveCompetitionAttemptCount(task) === 1, "one live attempt");
  const result = competitionForTask(task, {
    "tab-a": { id: "tab-a", pty_id: null, tab_type: "Terminal", title: "Worker A", cwd: null } as never,
  });
  assert(result?.liveCount === 1, "live count on result");
  assert(result?.tooltip.includes("live: Worker A"), "live attempt label");
  assert(result?.tooltip.includes("lane 1"), "lane in tooltip");
  assert(!result?.tooltip.includes("Worker B"), "closed attempt omitted from live lines");
});

test("records no-live fallback when competition is enabled but idle", () => {
  const task: Task = {
    ...FIXTURE_TASK,
    competition: { enabled: true },
  };
  const result = competitionForTask(task);
  assert(result?.tooltip.includes("no live attempts"), "idle fallback");
});

let passed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    passed += 1;
    console.log(`  ✓ ${name}`);
  } catch (error) {
    console.error(`  ✗ ${name}`);
    console.error(error);
    process.exitCode = 1;
    break;
  }
}
console.log(`\n✓ ${passed}/${tests.length} competitionForTask tests passed`);
