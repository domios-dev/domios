// Run: npx tsx src/components/Canvas/missionControl/missionControlInputRevision.test.ts

import type { Pane } from "../../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task } from "../types";
import { missionControlInputRevision } from "./missionControlInputRevision";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const PANE: Pane = {
  id: "pane-a",
  pty_id: "pty-a",
  pane_type: "Terminal",
  title: "Worker",
  cwd: "/repo",
};

function mkPlan(tasks: Task[]): Plan {
  return { ...FIXTURE_PLAN, tasks, loops: [] };
}

test("revision stable when only unrelated plan fields change", () => {
  const plan = mkPlan([{ ...FIXTURE_TASK, id: "t1", status: TaskStatus.InProgress, milestones: [] }]);
  const a = missionControlInputRevision(plan, {});
  const touched = { ...plan, canvas: { ...plan.canvas, x: 99 } };
  assert(missionControlInputRevision(touched, {}) === a, "canvas coords ignored");
});

test("revision changes when task status changes", () => {
  const pending = mkPlan([{ ...FIXTURE_TASK, id: "t1", status: TaskStatus.Pending, milestones: [] }]);
  const live = mkPlan([{ ...FIXTURE_TASK, id: "t1", status: TaskStatus.InProgress, milestones: [] }]);
  assert(
    missionControlInputRevision(pending, {}) !== missionControlInputRevision(live, {}),
    "status churn",
  );
});

test("revision changes when Tab title changes", () => {
  const plan = mkPlan([
    {
      ...FIXTURE_TASK,
      id: "t1",
      status: TaskStatus.InProgress,
      assigned_pane_id: PANE.id,
      milestones: [],
    },
  ]);
  const panesA = { [PANE.id]: PANE };
  const panesB = { [PANE.id]: { ...PANE, title: "Renamed" } };
  assert(
    missionControlInputRevision(plan, panesA) !== missionControlInputRevision(plan, panesB),
    "Tab title",
  );
});

let passed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    passed++;
    console.log(`  ✓ ${name}`);
  } catch (e) {
    console.error(`  ✗ ${name}`);
    console.error(e);
    process.exit(1);
  }
}
console.log(`\n✓ ${passed}/${tests.length} missionControlInputRevision tests passed`);
