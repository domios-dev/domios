// Run: npx tsx src/components/Canvas/planView/boardModel.test.ts

import { FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Section, type Task } from "../types";
import {
  BOARD_COLUMN_DEFS,
  boundDoneColumnCards,
  buildBoardPlanModel,
  deriveBoardPlanSummary,
  deriveBoardSpecSummary,
  DONE_COLUMN_COLLAPSED_LIMIT,
  groupTasksIntoColumns,
} from "./boardModel";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const PANES = {
  "pane-worker": { id: "pane-worker", title: "Puccini", agent_session_id: null } as never,
};

function task(overrides: Partial<Task>): Task {
  return { ...FIXTURE_TASK, ...overrides };
}

test("BOARD_COLUMN_DEFS covers workflow statuses in stable order", () => {
  assert(BOARD_COLUMN_DEFS.map((c) => c.key).join(",") === "pending,in_progress,in_review,blocked,done", "column keys");
});

test("deriveBoardSpecSummary returns null without primary_spec_id", () => {
  assert(deriveBoardSpecSummary(FIXTURE_PLAN, []) === null, "no spec");
});

test("deriveBoardSpecSummary counts fetched spec sections", () => {
  const specSections: Section[] = [
    { ...FIXTURE_SECTION, id: "sp-1", title: "North star", subsections: [] },
    { ...FIXTURE_SECTION, id: "sp-2", title: "Constraints", subsections: [] },
  ];
  const plan: Plan = { ...FIXTURE_PLAN, primary_spec_id: "spec-1" };
  const summary = deriveBoardSpecSummary(plan, specSections);
  assert(summary?.specId === "spec-1", "spec id");
  assert(summary?.sectionCount === 2, `section count=${summary?.sectionCount}`);
  assert(summary?.leadSection === "North star", "lead section");
});

test("deriveBoardPlanSummary tracks done/total and focus", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    heartbeat: { current_focus: "task:task-dante auth lane" },
    goal: { vector: "Ship auth safely" },
    tasks: [
      task({ id: "t1", status: TaskStatus.Done }),
      task({ id: "t2", status: TaskStatus.InProgress }),
      task({ id: "t3", status: TaskStatus.Cancelled }),
    ],
  };
  const summary = deriveBoardPlanSummary(plan, PANES);
  assert(summary.done === 1 && summary.total === 2, "progress excludes cancelled");
  assert(summary.focus?.includes("task:task-dante"), "focus");
  assert(summary.goalVector === "Ship auth safely", "goal");
});

test("groupTasksIntoColumns places tasks by status and sorts freshest first", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({ id: "old", status: TaskStatus.Pending, status_changed_at: 1 }),
      task({ id: "new", status: TaskStatus.Pending, status_changed_at: 99 }),
      task({ id: "live", status: TaskStatus.InProgress, assigned_pane_id: "pane-worker" }),
      task({ id: "gone", status: TaskStatus.Cancelled }),
    ],
  };
  const { columns, cancelledCount } = groupTasksIntoColumns(plan, PANES);
  assert(cancelledCount === 1, "cancelled tracked");
  const pending = columns.find((c) => c.key === "pending")!;
  assert(pending.cards.map((c) => c.task.id).join(",") === "new,old", "pending sort");
  const live = columns.find((c) => c.key === "in_progress")!;
  assert(live.cards[0]?.presenceLabel === "Puccini", "presence from pane title");
  assert(!live.cards[0]?.historicalExecutorSummary, "active task has no historical summary");
});

test("done task shows historical executor without active presence label", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "done",
        status: TaskStatus.Done,
        assigned_pane_id: "pane-worker",
        taken_by_role: "backend-dev",
        model: "gpt-5.5",
        lane: { pane_id: "pane-worker", branch: "feat/history" },
      }),
    ],
  };
  const card = groupTasksIntoColumns(plan, PANES).columns.find((c) => c.key === "done")!.cards[0]!;
  assert(card.presenceLabel === null, "done task has no active presence");
  assert(card.historicalExecutorSummary?.includes("Puccini"), "historical summary uses pane title");
  assert(card.historicalExecutorSummary?.includes("backend-dev"), "historical summary includes role");
});

test("buildBoardPlanModel wires spec + columns", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    primary_spec_id: "spec-abc",
    tasks: [task({ id: "t-blocked", status: TaskStatus.Blocked, name: "Blocked lane" })],
  };
  const model = buildBoardPlanModel(plan, PANES, [FIXTURE_SECTION]);
  assert(model.spec?.specId === "spec-abc", "spec summary");
  assert(model.columns.find((c) => c.key === "blocked")?.cards[0]?.task.name === "Blocked lane", "blocked card");
});

test("boundDoneColumnCards keeps newest N when collapsed", () => {
  const cards = Array.from({ length: 8 }, (_, i) =>
    task({ id: `done-${i}`, status: TaskStatus.Done, status_changed_at: 100 - i }),
  ).map((t) => ({
    task: t,
    presenceLabel: null,
    historicalExecutorSummary: null,
    roleLabel: null,
    branch: null,
    worktree: null,
    workspaceName: null,
    latestMilestone: null,
    latestMilestoneKind: null,
    focusMatch: false,
  }));
  const collapsed = boundDoneColumnCards(cards, false);
  assert(collapsed.total === 8, "total count");
  assert(collapsed.hidden === 3, `hidden=${collapsed.hidden}`);
  assert(collapsed.visible.length === DONE_COLUMN_COLLAPSED_LIMIT, "visible capped");
  assert(collapsed.visible[0]?.task.id === "done-0", "newest first");

  const expanded = boundDoneColumnCards(cards, true);
  assert(expanded.visible.length === 8 && expanded.hidden === 0, "expanded shows all");
});

test("boundDoneColumnCards passes through small done columns", () => {
  const cards = [task({ id: "only", status: TaskStatus.Done })].map((t) => ({
    task: t,
    presenceLabel: null,
    historicalExecutorSummary: null,
    roleLabel: null,
    branch: null,
    worktree: null,
    workspaceName: null,
    latestMilestone: null,
    latestMilestoneKind: null,
    focusMatch: false,
  }));
  const bounds = boundDoneColumnCards(cards, false);
  assert(bounds.visible.length === 1 && bounds.hidden === 0, "no bounding needed");
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
console.log(`\n✓ ${passed}/${tests.length} boardModel tests passed`);
