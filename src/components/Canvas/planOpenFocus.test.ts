// Run: npx tsx src/components/Canvas/planOpenFocus.test.ts

import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import { assignedTaskIdsForPane, resolvePlanOpenCameraTarget } from "./planOpenFocus";

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
    { ...FIXTURE_TASK, id: "task-a", assigned_pane_id: "worker-1" },
    { ...FIXTURE_TASK, id: "task-b", assigned_pane_id: "orch-1" },
  ],
};

test("assignedTaskIdsForPane returns tasks bound to the pane", () => {
  assert(
    assignedTaskIdsForPane([plan], "worker-1").join() === "task-a",
    "worker task",
  );
  assert(assignedTaskIdsForPane([plan], "unknown").length === 0, "none for unknown pane");
});

test("assignedTaskIdsForPane ignores terminal and empty pane bindings", () => {
  const mixed = {
    ...FIXTURE_PLAN,
    tasks: [
      { ...FIXTURE_TASK, id: "done", status: "done", assigned_pane_id: "worker-1" },
      { ...FIXTURE_TASK, id: "cancelled", status: "cancelled", assigned_pane_id: "worker-1" },
      { ...FIXTURE_TASK, id: "empty", assigned_pane_id: " " },
      { ...FIXTURE_TASK, id: "active", status: "in_review", assigned_pane_id: " worker-1 " },
    ],
  };
  assert(assignedTaskIdsForPane([mixed], "worker-1").join() === "active", "only active trimmed binding remains");
  assert(assignedTaskIdsForPane([mixed], " ").length === 0, "empty target pane ignored");
});

test("resolvePlanOpenCameraTarget prefers task focus in task context", () => {
  const target = resolvePlanOpenCameraTarget([plan], "worker-1", plan.id);
  assert(target.kind === "task", "task context");
  if (target.kind === "task") assert(target.nodeIds.join() === "task-a", "worker task id");
});

test("resolvePlanOpenCameraTarget frames active plan when pane has no assigned tasks", () => {
  const target = resolvePlanOpenCameraTarget([plan], "orch-no-tasks", plan.id);
  assert(target.kind === "plan", "normal open");
  if (target.kind === "plan") assert(target.nodeIds[0] === plan.id, "active plan id");
});

test("resolvePlanOpenCameraTarget falls back to bounds without focus pane or plan", () => {
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
