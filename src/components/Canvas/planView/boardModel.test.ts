// Run: npx tsx src/components/Canvas/planView/boardModel.test.ts

import type { Team, Workspace } from "../../../lib/types";
import { FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Section, type Task } from "../types";
import {
  BOARD_COLUMN_DEFS,
  boundDoneColumnCards,
  buildBoardPlanModel,
  deriveBoardPlanSummary,
  deriveBoardSpecSummary,
  DONE_COLUMN_COLLAPSED_LIMIT,
  formatRelativeAge,
  groupTasksIntoColumns,
  orderPlansByActivity,
  planActivityFreshness,
  resolveDisplayPlanningState,
  selectDefaultPlanId,
  type BoardTaskCardModel,
} from "./boardModel";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const TABS = {
  "tab-worker": { id: "tab-worker", title: "Puccini", agent_session_id: null } as never,
};

function task(overrides: Partial<Task>): Task {
  return { ...FIXTURE_TASK, ...overrides };
}

function doneCard(t: Task): BoardTaskCardModel {
  return {
    task: t,
    presenceLabel: null,
    historicalExecutorSummary: null,
    roleLabel: null,
    branch: null,
    latestNote: null,
    workingOn: null,
    loopBadge: null,
    competeBadge: null,
    focusMatch: false,
    locator: null,
  };
}

test("BOARD_COLUMN_DEFS covers workflow statuses in stable order", () => {
  assert(
    BOARD_COLUMN_DEFS.map((c) => c.key).join(",")
      === "pending,in_progress,in_review,blocked,paused,done",
    "column keys",
  );
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
  const summary = deriveBoardPlanSummary(plan, TABS);
  assert(summary.done === 1 && summary.total === 2, "progress excludes cancelled");
  assert(summary.focus?.includes("task:task-dante"), "focus");
  assert(summary.goalVector === "Ship auth safely", "goal");
});

test("resolveDisplayPlanningState bridges drafting default on execution plans", () => {
  const plan: Plan = { ...FIXTURE_PLAN, mode: "execution", planning_state: "drafting" };
  assert(resolveDisplayPlanningState(plan) === "approved", "execution never shows drafting");
});

test("resolveDisplayPlanningState omits chip on complete plans with drafting default", () => {
  const plan: Plan = { ...FIXTURE_PLAN, mode: "complete", planning_state: "drafting" };
  assert(resolveDisplayPlanningState(plan) === null, "complete hides drafting lie");
});

test("deriveBoardPlanSummary labels remote planner when Tab is not local", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    planner: { kind: "tab", tab_id: "planner-prod-99", title: null },
  };
  const summary = deriveBoardPlanSummary(plan, TABS);
  assert(summary.plannerLabel === "planner- (remote)", "remote short id label");
  assert(summary.plannerTabId === "planner-prod-99", "Tab id preserved");
});

test("deriveBoardPlanSummary prefers bound planner title over remote fallback", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    planner: { kind: "tab", tab_id: "planner-prod-99", title: "Prod planner" },
  };
  const summary = deriveBoardPlanSummary(plan, {});
  assert(summary.plannerLabel === "Prod planner", "ref title when Tab absent");
});

test("groupTasksIntoColumns exposes paused tasks in a distinct column", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({ id: "paused-a", status: TaskStatus.Paused, name: "Deferred auth" }),
      task({ id: "paused-b", status: TaskStatus.Paused, name: "Wait on API" }),
      task({ id: "pending", status: TaskStatus.Pending }),
    ],
  };
  const { columns } = groupTasksIntoColumns(plan, TABS);
  const paused = columns.find((c) => String(c.key) === "paused");
  assert(paused?.cards.length === 2, `paused count=${paused?.cards.length}`);
  const pending = columns.find((c) => c.key === "pending")!;
  assert(pending.cards.length === 1, `pending count=${pending.cards.length}`);
});

test("groupTasksIntoColumns places tasks by status in planned execution order", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({ id: "later", status: TaskStatus.Pending, created_at: 20, status_changed_at: 1 }),
      task({ id: "first", status: TaskStatus.Pending, created_at: 10, status_changed_at: 99 }),
      task({ id: "unsequenced", status: TaskStatus.Pending, created_at: undefined }),
      task({ id: "live", status: TaskStatus.InProgress, assigned_tab_id: "tab-worker" }),
      task({ id: "gone", status: TaskStatus.Cancelled }),
    ],
  };
  const { columns, cancelledCount } = groupTasksIntoColumns(plan, TABS);
  assert(cancelledCount === 1, "cancelled tracked");
  const pending = columns.find((c) => c.key === "pending")!;
  assert(
    pending.cards.map((c) => c.task.id).join(",") === "first,later,unsequenced",
    "created order wins over recent activity, with unsequenced tasks last",
  );
  const live = columns.find((c) => c.key === "in_progress")!;
  assert(live.cards[0]?.presenceLabel === "Puccini", "presence from Tab title");
  assert(!live.cards[0]?.historicalExecutorSummary, "active task has no historical summary");
});

test("groupTasksIntoColumns keeps canonical status separate from milestone note", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "qa-one",
        status: TaskStatus.InReview,
        review_stage: "qa",
        qa_state: "running",
        status_label: "In QA · running",
        milestones: [
          {
            id: "ms-qa",
            from: { tab_id: "tab-worker", session_id: null, title: "Puccini" },
            kind: "decision",
            text: "Queued behind auth lane",
            ts: 2_000,
            links: [],
            loop_id: null,
            iteration: null,
            canvas: { x: null, y: null, collapsed: false },
          },
        ],
      }),
    ],
  };
  const card = groupTasksIntoColumns(plan, TABS).columns.find((c) => c.key === "in_review")!.cards[0]!;
  assert(card.task.status === TaskStatus.InReview, "status field unchanged");
  assert(card.latestNote === "Queued behind auth lane", "latest note from milestone text");
});

test("done task shows historical executor without active presence label", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "done",
        status: TaskStatus.Done,
        assigned_tab_id: "tab-worker",
        taken_by_role: "backend-dev",
        model: "gpt-5.5",
        lane: { tab_id: "tab-worker", branch: "feat/history" },
      }),
    ],
  };
  const card = groupTasksIntoColumns(plan, TABS).columns.find((c) => c.key === "done")!.cards[0]!;
  assert(card.presenceLabel === null, "done task has no active presence");
  assert(card.historicalExecutorSummary?.includes("Puccini"), "historical summary uses Tab title");
  assert(card.historicalExecutorSummary?.includes("backend-dev"), "historical summary includes role");
});

test("buildBoardPlanModel wires spec + columns", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    primary_spec_id: "spec-abc",
    tasks: [task({ id: "t-blocked", status: TaskStatus.Blocked, name: "Blocked lane" })],
  };
  const model = buildBoardPlanModel(plan, TABS, [FIXTURE_SECTION]);
  assert(model.spec?.specId === "spec-abc", "spec summary");
  assert(model.columns.find((c) => c.key === "blocked")?.cards[0]?.task.name === "Blocked lane", "blocked card");
});

test("boundDoneColumnCards preserves the column sequence when collapsed", () => {
  const cards = Array.from({ length: 8 }, (_, i) =>
    task({ id: `done-${i}`, status: TaskStatus.Done, status_changed_at: 100 - i }),
  ).map(doneCard);
  const collapsed = boundDoneColumnCards(cards, false);
  assert(collapsed.total === 8, "total count");
  assert(collapsed.hidden === 3, `hidden=${collapsed.hidden}`);
  assert(collapsed.visible.length === DONE_COLUMN_COLLAPSED_LIMIT, "visible capped");
  assert(collapsed.visible.map((card) => card.task.id).join(",") === "done-0,done-1,done-2,done-3,done-4", "sequence preserved");

  const expanded = boundDoneColumnCards(cards, true);
  assert(expanded.visible.length === 8 && expanded.hidden === 0, "expanded shows all");
});

test("boundDoneColumnCards passes through small done columns", () => {
  const cards = [task({ id: "only", status: TaskStatus.Done })].map(doneCard);
  const bounds = boundDoneColumnCards(cards, false);
  assert(bounds.visible.length === 1 && bounds.hidden === 0, "no bounding needed");
});

test("task card carries branch-only lane and latest note, never lane worktree paths", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "t-lane",
        status: TaskStatus.InProgress,
        assigned_tab_id: "tab-worker",
        lane: { branch: "lane/board-v2", worktree_path: "/tmp/codeterm-board-v2", workspace_name: "board-v2" },
        milestones: [
          {
            id: "ms",
            from: { tab_id: "tab-worker", session_id: null, title: "Puccini" },
            kind: "status_update",
            text: "secret worktree /tmp/codeterm-board-v2 detail",
            ts: 1_000,
            links: [],
            loop_id: null,
            iteration: null,
            canvas: { x: null, y: null, collapsed: false },
          },
        ],
      }),
    ],
  };
  const card = groupTasksIntoColumns(plan, TABS, 61_000).columns.find((c) => c.key === "in_progress")!.cards[0]!;
  assert(card.branch === "lane/board-v2", "branch chip is branch-only");
  const { task: _t, ...derived } = card;
  const serialized = JSON.stringify(derived);
  assert(!serialized.includes("worktree_path"), "lane worktree path not in derived card fields");
  assert(!("worktree" in card) && !("latestMilestone" in card), "no worktree/milestone object fields");
  assert(card.latestNote === "secret worktree /tmp/codeterm-board-v2 detail", "latest note carries milestone text");
});

test("formatRelativeAge buckets seconds/minutes/hours/days", () => {
  assert(formatRelativeAge(0, 5_000) === "5s", "seconds");
  assert(formatRelativeAge(0, 120_000) === "2m", "minutes");
  assert(formatRelativeAge(0, 7_200_000) === "2h", "hours");
  assert(formatRelativeAge(0, 172_800_000) === "2d", "days");
});

test("orderPlansByActivity sorts freshest first with deterministic tiebreak", () => {
  const stale: Plan = { ...FIXTURE_PLAN, id: "p-stale", title: "Stale", updated_at: 10, created_at: 10, tasks: [] };
  const fresh: Plan = {
    ...FIXTURE_PLAN,
    id: "p-fresh",
    title: "Fresh",
    updated_at: 20,
    created_at: 20,
    tasks: [task({ id: "tf", status: TaskStatus.InProgress, updated_at: 900 })],
  };
  const tieB: Plan = { ...FIXTURE_PLAN, id: "p-b", title: "Beta", updated_at: 50, created_at: 50, tasks: [] };
  const tieA: Plan = { ...FIXTURE_PLAN, id: "p-a", title: "Alpha", updated_at: 50, created_at: 50, tasks: [] };
  const ordered = orderPlansByActivity([stale, tieB, fresh, tieA]);
  assert(ordered[0]?.id === "p-fresh", "freshest first");
  assert(ordered[1]?.id === "p-a" && ordered[2]?.id === "p-b", "equal timestamps break by title");
  assert(planActivityFreshness(fresh) === 900, "freshness includes task stamps");
});

test("selectDefaultPlanId honors explicit id, then orchestrator ownership, then freshest", () => {
  const a: Plan = { ...FIXTURE_PLAN, id: "p-a", title: "Alpha", created_at: 0, updated_at: 5, tasks: [] };
  const b: Plan = { ...FIXTURE_PLAN, id: "p-b", title: "Beta", created_at: 0, updated_at: 99, tasks: [] };
  const ordered = orderPlansByActivity([a, b]);
  assert(selectDefaultPlanId(ordered, { workspaceId: undefined }) === "p-b", "freshest fallback");
  assert(selectDefaultPlanId(ordered, { planId: "p-a", workspaceId: undefined }) === "p-a", "explicit id wins");
  assert(selectDefaultPlanId([], { workspaceId: undefined }) === null, "empty -> null");
});

test("selectDefaultPlanId prefers a plan with real work over a fresh junk 0-task plan", () => {
  const junk: Plan = { ...FIXTURE_PLAN, id: "p-junk", title: "Junk", created_at: 0, updated_at: 999, tasks: [] };
  const bound: Plan = {
    ...FIXTURE_PLAN,
    id: "p-bound",
    title: "Bound",
    created_at: 0,
    updated_at: 10,
    tasks: [{ ...FIXTURE_TASK, id: "t1", status: TaskStatus.InProgress }],
  };
  const ordered = orderPlansByActivity([junk, bound]); // junk is freshest
  assert(ordered[0].id === "p-junk", "junk sorts first by freshness");
  assert(selectDefaultPlanId(ordered, { workspaceId: undefined }) === "p-bound", "but selector picks the plan with actionable work");
});

const WS_CURRENT = "ws-current";
const WS_OTHER = "ws-other";
const scopeWorkspaces: Workspace[] = [
  { id: WS_CURRENT, name: WS_CURRENT, tabs: ["orch-current"], active_tab_id: null, cwd: null },
  { id: WS_OTHER, name: WS_OTHER, tabs: ["orch-other"], active_tab_id: null, cwd: null },
];
const scopeTeams: Team[] = [
  { id: "team-other", lifecycle: "active", created_at: 0, workspace_id: WS_OTHER },
];

test("selectDefaultPlanId returns null when no plan is bound to the workspace (no fallback)", () => {
  const foreign: Plan = {
    ...FIXTURE_PLAN,
    id: "p-foreign",
    team_id: "team-other",
    owner: { tab_id: "orch-other", session_id: null, title: "Other" },
    tasks: [{ ...FIXTURE_TASK, id: "t1", status: TaskStatus.InProgress }],
  };
  const ordered = orderPlansByActivity([foreign]);
  const id = selectDefaultPlanId(ordered, {
    workspaceId: WS_CURRENT,
    workspaces: scopeWorkspaces,
    teams: scopeTeams,
  });
  assert(id === null, `no workspace-bound plan → null default, got ${id}`);
});

test("selectDefaultPlanId still honors explicit planId under workspace scope", () => {
  const foreign: Plan = {
    ...FIXTURE_PLAN,
    id: "p-foreign",
    team_id: "team-other",
    owner: { tab_id: "orch-other", session_id: null, title: "Other" },
    tasks: [],
  };
  const ordered = orderPlansByActivity([foreign]);
  const id = selectDefaultPlanId(ordered, {
    planId: "p-foreign",
    workspaceId: WS_CURRENT,
    workspaces: scopeWorkspaces,
    teams: scopeTeams,
  });
  assert(id === "p-foreign", `explicit planId wins even across workspace scope, got ${id}`);
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
