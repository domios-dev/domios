// Run: npx tsx src/components/Canvas/planView/taskStatusLabel.test.ts

import { FIXTURE_MILESTONE, FIXTURE_TASK } from "../cards/fixtures.testdata";
import type { Plan } from "../types";
import { canonicalTaskStatusLabel, latestTaskNote } from "./taskStatusLabel";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("canonicalTaskStatusLabel ignores status_label", () => {
  assert(
    canonicalTaskStatusLabel({ ...FIXTURE_TASK, status: "in_review", status_label: "In QA · queued #1" }) === "in review",
    "status from task.status only",
  );
});

test("parent status rolls up open children and clears when all are terminal", () => {
  const parent = { ...FIXTURE_TASK, id: "parent", status: "in_progress" as const };
  const child = { ...FIXTURE_TASK, id: "child", parent_task_id: "parent", status: "pending" as const };
  const plan = { tasks: [parent, child] } as Plan;
  assert(canonicalTaskStatusLabel(parent, plan) === "blocked on subtasks", "open child blocks parent presentation");
  plan.tasks[1] = { ...child, status: "done" };
  assert(canonicalTaskStatusLabel(parent, plan) === "in progress", "terminal children clear parent presentation");
});

test("open children preserve review and terminal parent statuses", () => {
  const child = { ...FIXTURE_TASK, parent_task_id: "parent", status: "pending" as const };
  const plan = { tasks: [{ ...FIXTURE_TASK, id: "parent", status: "in_review" as const }, child] } as Plan;
  assert(canonicalTaskStatusLabel(plan.tasks[0], plan) === "in review", "review presentation stays visible");
  plan.tasks[0] = { ...plan.tasks[0], status: "cancelled" };
  assert(canonicalTaskStatusLabel(plan.tasks[0], plan) === "cancelled", "terminal presentation stays visible");
});

test("latestTaskNote picks newest milestone and truncates", () => {
  const note = latestTaskNote({
    ...FIXTURE_TASK,
    milestones: [
      { ...FIXTURE_MILESTONE, id: "old", ts: 1, text: "older" },
      { ...FIXTURE_MILESTONE, id: "new", ts: 9, text: "newest note" },
    ],
  });
  assert(note === "newest note", "newest milestone text");
  const long = "x".repeat(130);
  const truncated = latestTaskNote({
    ...FIXTURE_TASK,
    milestones: [{ ...FIXTURE_MILESTONE, text: long }],
  });
  assert(truncated && truncated.length === 120 && truncated.endsWith("…"), "truncates long notes");
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
