// Run: npx tsx src/components/Canvas/planView/planListModel.test.ts

import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan } from "../types";
import { buildPlanListRows } from "./buildPlanListTree";
import {
  DONE_LIST_FILTER_VISIBLE_LIMIT,
  boundDoneFilterVisibleRows,
  buildVisiblePlanListRows,
} from "./planListModel";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function planWithDoneTasks(count: number): Plan {
  const tasks = Array.from({ length: count }, (_, i) => ({
    ...FIXTURE_TASK,
    id: `t-done-${i}`,
    name: `Done ${i}`,
    status: TaskStatus.Done,
    status_changed_at: count - i,
    updated_at: count - i,
  }));
  return { ...FIXTURE_PLAN, tasks };
}

function visibleDoneTaskIds(visible: ReturnType<typeof buildVisiblePlanListRows>): string[] {
  return visible
    .filter((entry) => entry.type === "row" && entry.row.taskId)
    .map((entry) => (entry as { type: "row"; row: { taskId?: string } }).row.taskId!)
    .filter(Boolean);
}

test("done filter keeps small done sets fully visible", () => {
  const rows = buildPlanListRows(planWithDoneTasks(3), {});
  const visible = buildVisiblePlanListRows(rows, {
    statusFilter: "done",
    textQuery: "",
    expansionOverrides: new Map(),
  });
  const capped = boundDoneFilterVisibleRows(visible, false);
  assert(visibleDoneTaskIds(capped).length === 3, "all three done tasks visible");
  assert(!capped.some((entry) => entry.type === "done-filter-toggle"), "no toggle under cap");
});

test("done filter caps 250 done tasks to recent limit with show-all toggle", () => {
  const rows = buildPlanListRows(planWithDoneTasks(250), {});
  const visible = buildVisiblePlanListRows(rows, {
    statusFilter: "done",
    textQuery: "",
    expansionOverrides: new Map(),
  });
  assert(visibleDoneTaskIds(visible).length === 250, "uncapped model lists every done task");

  const capped = boundDoneFilterVisibleRows(visible, false);
  const taskIds = visibleDoneTaskIds(capped);
  assert(taskIds.length === DONE_LIST_FILTER_VISIBLE_LIMIT, `capped to ${DONE_LIST_FILTER_VISIBLE_LIMIT}, got ${taskIds.length}`);
  assert(taskIds[0] === "t-done-0", "most recent done task first");
  const toggle = capped.find((entry) => entry.type === "done-filter-toggle");
  assert(toggle?.type === "done-filter-toggle", "show-all toggle present");
  assert(toggle.totalCount === 250, "toggle reports full count");
  assert(toggle.hiddenCount === 250 - DONE_LIST_FILTER_VISIBLE_LIMIT, "toggle reports hidden count");
});

test("done filter expand toggle reveals all done tasks", () => {
  const rows = buildPlanListRows(planWithDoneTasks(250), {});
  const visible = buildVisiblePlanListRows(rows, {
    statusFilter: "done",
    textQuery: "",
    expansionOverrides: new Map(),
  });
  const collapsed = boundDoneFilterVisibleRows(visible, false);
  const expanded = boundDoneFilterVisibleRows(visible, true);
  assert(visibleDoneTaskIds(expanded).length === 250, "expanded shows all done tasks");
  assert(
    expanded.some((entry) => entry.type === "done-filter-toggle" && entry.expanded),
    "collapse toggle present when expanded",
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
console.log(`\n✓ ${passed}/${tests.length} planListModel tests passed`);
