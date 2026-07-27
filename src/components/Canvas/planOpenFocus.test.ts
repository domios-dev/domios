// Run: npx tsx src/components/Canvas/planOpenFocus.test.ts

import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import { assignedTaskIdsForTab, resolvePlanOpenCameraTarget } from "./planOpenFocus";
import { TaskStatus } from "./types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const plan = {
  ...FIXTURE_PLAN,
  tasks: [
    { ...FIXTURE_TASK, id: "task-a", assigned_tab_id: "worker-1" },
    { ...FIXTURE_TASK, id: "task-b", assigned_tab_id: "orch-1" },
  ],
};

test("assignedTaskIdsForTab returns tasks bound to the Tab", () => {
  assert(
    assignedTaskIdsForTab([plan], "worker-1").join() === "task-a",
    "worker task",
  );
  assert(assignedTaskIdsForTab([plan], "unknown").length === 0, "none for unknown Tab");
});

test("assignedTaskIdsForTab ignores terminal and empty Tab bindings", () => {
  const mixed = {
    ...FIXTURE_PLAN,
    tasks: [
      { ...FIXTURE_TASK, id: "done", status: TaskStatus.Done, assigned_tab_id: "worker-1" },
      { ...FIXTURE_TASK, id: "cancelled", status: TaskStatus.Cancelled, assigned_tab_id: "worker-1" },
      { ...FIXTURE_TASK, id: "empty", assigned_tab_id: " " },
      { ...FIXTURE_TASK, id: "active", status: TaskStatus.InReview, assigned_tab_id: " worker-1 " },
    ],
  };
  assert(assignedTaskIdsForTab([mixed], "worker-1").join() === "active", "only active trimmed binding remains");
  assert(assignedTaskIdsForTab([mixed], " ").length === 0, "empty target Tab ignored");
});

test("resolvePlanOpenCameraTarget prefers task focus in task context", () => {
  const target = resolvePlanOpenCameraTarget([plan], "worker-1", plan.id);
  assert(target.kind === "task", "task context");
  if (target.kind === "task") assert(target.nodeIds.join() === "task-a", "worker task id");
});

test("resolvePlanOpenCameraTarget frames active plan when Tab has no assigned tasks", () => {
  const target = resolvePlanOpenCameraTarget([plan], "orch-no-tasks", plan.id);
  assert(target.kind === "plan", "normal open");
  if (target.kind === "plan") assert(target.nodeIds[0] === plan.id, "active plan id");
});

test("resolvePlanOpenCameraTarget falls back to bounds without focus Tab or plan", () => {
  assert(resolvePlanOpenCameraTarget([], null, null).kind === "bounds", "empty board");
});

async function main() {
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} planOpenFocus tests passed`);
}

main();
