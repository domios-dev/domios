// Run: npx tsx src/components/Canvas/planView/taskCardMetadata.test.ts

import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task } from "../types";
import { buildBoardPlanModel } from "./boardModel";
import { deriveTaskCardMetadata } from "./taskCardMetadata";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function task(overrides: Partial<Task>): Task {
  return { ...FIXTURE_TASK, ...overrides };
}

function cardFor(plan: Plan, taskId: string) {
  const model = buildBoardPlanModel(plan, {});
  for (const column of model.columns) {
    const found = column.cards.find((c) => c.task.id === taskId);
    if (found) return found;
  }
  throw new Error(`card not found: ${taskId}`);
}

test("done task with ghost tab never shows working-on or tab-gone", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "t-done",
        status: TaskStatus.Done,
        assigned_tab_id: "ghost-tab",
        lane: { branch: "lane/merged" },
        status_changed_at: 1_000,
      }),
    ],
  };
  const card = cardFor(plan, "t-done");
  assert(card.workingOn === null, "no working-on row");
  const meta = deriveTaskCardMetadata(card, 10_000);
  assert(!meta.rows.some((r) => /tab gone/i.test(r.value)), "no tab-gone on done");
  assert(meta.rows.some((r) => r.kind === "branch" && r.label === "Branch"), "branch row");
  assert(meta.rows.some((r) => r.kind === "age" && r.label === "Age"), "age row separate");
});

test("pending task shows labeled handle not working-on", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [task({ id: "t-pend", status: TaskStatus.Pending, handle: "my-handle" })],
  };
  const card = cardFor(plan, "t-pend");
  assert(card.workingOn === null, "no liveness on pending");
  const meta = deriveTaskCardMetadata(card);
  assert(meta.rows.length === 1 && meta.rows[0]?.kind === "handle", "handle row");
  assert(meta.rows[0]?.label === "Handle", "labeled handle");
});

test("in_progress uses working-on not metadata rows", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "t-live",
        status: TaskStatus.InProgress,
        assigned_tab_id: "p1",
        lane: { branch: "lane/x" },
      }),
    ],
  };
  const card = cardFor(plan, "t-live");
  assert(card.workingOn != null, "working-on present");
  assert(deriveTaskCardMetadata(card).rows.length === 0, "no secondary rows");
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
console.log(`\n${passed}/${tests.length} passed`);
