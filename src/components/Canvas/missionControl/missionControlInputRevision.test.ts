// Run: npx tsx src/components/Canvas/missionControl/missionControlInputRevision.test.ts

import type { Tab } from "../../../lib/types";
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

const TAB: Tab = {
  id: "tab-a",
  pty_id: "pty-a",
  tab_type: "Terminal",
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

test("revision is stable for identical inputs and changes when assigned Tab presence changes", () => {
  const plan = mkPlan([
    {
      ...FIXTURE_TASK,
      id: "t1",
      status: TaskStatus.InProgress,
      assigned_tab_id: TAB.id,
      milestones: [],
    },
  ]);
  const tabs = { [TAB.id]: TAB };
  const revision = missionControlInputRevision(plan, tabs);
  assert(
    missionControlInputRevision(plan, tabs) === revision,
    "identical inputs produce a stable revision",
  );
  assert(
    missionControlInputRevision(plan, {}) !== revision,
    "assigned Tab presence changes the revision",
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
