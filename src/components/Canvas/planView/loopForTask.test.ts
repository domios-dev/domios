// Run: npx tsx src/components/Canvas/planView/loopForTask.test.ts

import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import type { LoopSpec, Plan, Task } from "../types";
import {
  LOOP_BADGE_MIN_COUNT,
  LOOP_STAGNATION_THRESHOLD,
  isActiveLoop,
  isLoopStagnating,
  loopForTask,
  loopTargetsTask,
} from "./loopForTask";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function runningLoop(overrides: Partial<LoopSpec> = {}): LoopSpec {
  return {
    id: "loop-a",
    target: FIXTURE_TASK.id,
    budget: 5,
    count: 2,
    stop_when: "tests green",
    status: "running",
    stagnation: 0,
    ...overrides,
  };
}

test("returns null when no active loop targets the task", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    loops: [{ ...runningLoop(), status: "done", count: 4 }],
  };
  assert(loopForTask(plan, FIXTURE_TASK.id) === null, "terminal loop hidden");
});

test("returns null when aggregated count is below threshold", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    loops: [runningLoop({ count: 1 })],
  };
  assert(loopForTask(plan, FIXTURE_TASK.id) === null, "count < 2 hidden");
});

test("aggregates multiple active loops without double-counting iterations", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    loops: [
      runningLoop({ id: "loop-a", count: 2 }),
      runningLoop({ id: "loop-b", count: 3 }),
    ],
  };
  const result = loopForTask(plan, FIXTURE_TASK.id);
  assert(result?.count === 5, "sum loop counts");
  assert(result?.loops.length === 2, "both loops retained");
  assert(result?.tooltip.includes("loop-a"), "tooltip lists first loop");
  assert(result?.tooltip.includes("loop-b"), "tooltip lists second loop");
});

test("matches loop target by task handle", () => {
  const task: Task = { ...FIXTURE_TASK, handle: "auth-login" };
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [task],
    loops: [runningLoop({ target: "auth-login", count: 2 })],
  };
  assert(loopTargetsTask(plan.loops[0], task), "handle match");
  assert(loopForTask(plan, task.id)?.count === 2, "resolved via handle target");
});

test("warns when count reaches budget or loop is stagnating", () => {
  const atBudget = loopForTask(
    { ...FIXTURE_PLAN, loops: [runningLoop({ count: 5, budget: 5 })] },
    FIXTURE_TASK.id,
  );
  assert(atBudget?.warning === true, "budget warning");

  const stagnating = loopForTask(
    {
      ...FIXTURE_PLAN,
      loops: [runningLoop({ count: 2, stagnation: LOOP_STAGNATION_THRESHOLD })],
    },
    FIXTURE_TASK.id,
  );
  assert(stagnating?.warning === true, "stagnation warning");
  assert(stagnating?.tooltip.includes("stagnating"), "stagnation in tooltip");
});

test("does not infer looping from milestone text", () => {
  const task: Task = {
    ...FIXTURE_TASK,
    milestones: [{
      id: "ms-loop",
      from: { tab_id: "p1", session_id: null, title: "Worker" },
      kind: "status_update",
      text: "loop iteration 9 recorded in milestone only",
      ts: 1,
      links: [],
      loop_id: "ghost",
      iteration: 9,
      canvas: { x: null, y: null, collapsed: false },
    }],
  };
  const plan: Plan = { ...FIXTURE_PLAN, tasks: [task], loops: [] };
  assert(loopForTask(plan, task.id) === null, "milestones do not create loop badge");
});

test("helper predicates mirror active/stagnation rules", () => {
  assert(isActiveLoop(runningLoop()), "running loop active");
  assert(!isActiveLoop(runningLoop({ status: "stopped" })), "stopped inactive");
  assert(isLoopStagnating(runningLoop({ stagnation: LOOP_STAGNATION_THRESHOLD })), "stagnating");
  assert(!isLoopStagnating(runningLoop({ stagnation: LOOP_STAGNATION_THRESHOLD - 1 })), "not yet stagnating");
  assert(LOOP_BADGE_MIN_COUNT === 2, "documented threshold");
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
console.log(`\n✓ ${passed}/${tests.length} loopForTask tests passed`);
