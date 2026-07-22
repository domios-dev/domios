// Run: npx tsx src/components/Canvas/missionControl/missionControlClick.test.ts

import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task } from "../types";
import { buildMissionControl } from "./missionControl";
import { modelToSnapshot } from "./modelAdapter";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function mkPlan(tasks: Task[]): Plan {
  return { ...FIXTURE_PLAN, tasks, loops: [] };
}

function collectTaskIds(snap: ReturnType<typeof modelToSnapshot>): Set<string> {
  const ids = new Set<string>();
  for (const t of snap.dispatchQueue) ids.add(t.id);
  for (const t of snap.gateQueue) ids.add(t.id);
  for (const t of snap.qaQueue) ids.add(t.id);
  if (snap.qaRunning) ids.add(snap.qaRunning.id);
  for (const t of snap.doneRecent) ids.add(t.id);
  for (const t of snap.blocked) ids.add(t.id);
  for (const t of snap.paused) ids.add(t.id);
  for (const lane of snap.lanes) {
    for (const t of lane.activeTasks ?? (lane.activeTask ? [lane.activeTask] : [])) ids.add(t.id);
  }
  return ids;
}

test("every snapshot task id resolves in source plan for click routing", () => {
  const plan = mkPlan([
    { ...FIXTURE_TASK, id: "task-open", status: TaskStatus.Pending, milestones: [] },
    { ...FIXTURE_TASK, id: "task-wip", status: TaskStatus.InProgress, milestones: [], status_changed_at: Date.now() },
    { ...FIXTURE_TASK, id: "task-done", status: TaskStatus.Done, milestones: [] },
  ]);
  const model = buildMissionControl(plan, {});
  const snap = modelToSnapshot({ plan, model });
  const ids = collectTaskIds(snap);
  for (const id of ids) {
    const found = plan.tasks.find((t) => t.id === id);
    assert(found != null, `missing plan task for ${id}`);
  }
});

test("task card test ids match plan task ids", () => {
  const plan = mkPlan([{ ...FIXTURE_TASK, id: "task-click-me", status: TaskStatus.Pending, milestones: [] }]);
  const model = buildMissionControl(plan, {});
  const snap = modelToSnapshot({ plan, model });
  assert(snap.dispatchQueue[0]!.id === "task-click-me", "id preserved for data-testid=task-*");
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
console.log(`\n✓ ${passed}/${tests.length} missionControlClick tests passed`);
