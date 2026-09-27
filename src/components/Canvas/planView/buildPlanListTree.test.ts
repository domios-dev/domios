// Run: npx tsx src/components/Canvas/planView/buildPlanListTree.test.ts

import type { Tab } from "../../../lib/types";
import { FIXTURE_MILESTONE, FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { bandedSeedPlans } from "../layout/fixtures";
import { buildPlanListForest, buildPlanListRows } from "./buildPlanListTree";
import {
  buildVisiblePlanListRows,
  defaultSectionExpanded,
  type PlanListStatusFilter,
} from "./planListModel";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const tabs: Record<string, Tab> = {
  "tab-42": {
    id: "tab-42",
    pty_id: null,
    tab_type: "Terminal",
    title: "Puccini",
    cwd: "/tmp",
    agent_type: "codex",
  },
};

test("orders flat status groups before freshness-sorted task rows", () => {
  const [plan] = bandedSeedPlans();
  const rows = buildPlanListRows(plan, { tabs });
  const kinds = rows.map((r) => r.kind as string);
  const groupTitles = rows.filter((r) => r.kind === "plan-section" || r.kind === "unsectioned").map((r) => r.title);
  assert(groupTitles.slice(0, 4).join("|") === "In progress + review|Blocked|Pending|Done", "status group order");
  const activeStart = rows.findIndex((r) => r.title === "In progress + review");
  const activeTasks = rows.slice(activeStart + 1, rows.findIndex((r) => r.title === "Blocked")).filter((r) => r.taskId);
  assert(activeTasks[0]?.taskId === "t_bulk_9", "freshest active task first");
  assert(activeTasks.some((r) => r.kind === "subtask"), "subtasks still identified");
  assert(!kinds.includes("spec-root"), "no spec root band");
  assert(!kinds.includes("plan-root"), "no plan root band");
  assert(!kinds.includes("spec-section"), "no spec tree in list");
});

test("marks subtasks while keeping flat freshness order", () => {
  const [plan] = bandedSeedPlans();
  const rows = buildPlanListRows(plan, { tabs });
  const parentIdx = rows.findIndex((r) => r.taskId === "t_parent");
  const subAIdx = rows.findIndex((r) => r.taskId === "t_sub_a");
  assert(parentIdx >= 0 && subAIdx >= 0, "parent and child present");
  assert(rows[subAIdx].kind === "subtask", "child row kind");
  assert(rows[subAIdx].depth > rows[parentIdx].depth, "subtask indented deeper");
});

test("task rows carry belonging section title", () => {
  const [plan] = bandedSeedPlans();
  const rows = buildPlanListRows(plan, { tabs });
  const inbox = rows.find((r) => r.taskId === "t_inbox");
  assert(inbox && inbox.kind === "task", "inbox task listed");
  assert(inbox.sectionTitle === "sec_inbox", "section chip title");
});

test("unsectioned belonging for tasks without section_ids", () => {
  const plan = {
    ...FIXTURE_PLAN,
    tasks: [{ ...FIXTURE_TASK, section_ids: [] }],
  };
  const rows = buildPlanListRows(plan, { tabs });
  const taskRow = rows.find((r) => r.taskId === FIXTURE_TASK.id);
  assert(taskRow?.sectionTitle === "Unsectioned", "unsectioned chip");
});

test("surfaces tab label and milestone on active assigned task", () => {
  const plan = {
    ...FIXTURE_PLAN,
    heartbeat: { current_focus: "Ship auth", active_lanes: ["task:task-dante"] },
    tasks: [
      {
        ...FIXTURE_TASK,
        assigned_tab_id: "tab-42",
        milestones: [
          {
            ...FIXTURE_MILESTONE,
            from: { tab_id: "tab-42", session_id: null, title: null },
            kind: "status_update",
          },
        ],
      },
    ],
  };
  const taskRow = buildPlanListRows(plan, { tabs }).find((r) => r.taskId === FIXTURE_TASK.id);
  assert(taskRow?.tabLabel === "Puccini", "Tab title");
  assert(!taskRow?.historicalExecutorSummary, "active task has no historical summary");
  assert(taskRow?.milestoneKind === "status update", "milestone kind label");
});

test("task rows use canonical status labels", () => {
  const plan = {
    ...FIXTURE_PLAN,
    tasks: [
      {
        ...FIXTURE_TASK,
        id: "qa-b",
        status: "in_review" as const,
        review_stage: "qa" as const,
        qa_state: "queued" as const,
        status_label: "In QA · queued #2",
      },
    ],
  };
  const rows = buildPlanListRows(plan, { tabs });
  assert(rows.find((row) => row.taskId === "qa-b")?.statusLabel === "in review", "canonical status label");
});

test("unplanned task badge carries its reason label", () => {
  const task = {
    ...FIXTURE_TASK,
    name: "unplanned: urgent probe",
    tags: ["unplanned"],
  };
  const plan = { ...FIXTURE_PLAN, tasks: [task] };
  const row = buildPlanListRows(plan, { tabs }).find((candidate) => candidate.taskId === task.id);
  assert(row?.statusLabel === "unplanned: urgent probe", "unplanned badge includes its reason");
});

test("done task surfaces historical executor instead of active tab label", () => {
  const plan = {
    ...FIXTURE_PLAN,
    tasks: [
      {
        ...FIXTURE_TASK,
        status: "done" as const,
        assigned_tab_id: "tab-42",
        taken_by_role: "backend-dev",
        model: "gpt-5.5",
        lane: { tab_id: "tab-42", branch: "feat/history" },
      },
    ],
  };
  const taskRow = buildPlanListRows(plan, { tabs }).find((r) => r.taskId === FIXTURE_TASK.id);
  assert(!taskRow?.tabLabel, "done task has no active tab label");
  assert(taskRow?.historicalExecutorSummary?.includes("Puccini"), "historical summary shown");
});

test("task-first list carries plan sections but no spec tree rows", () => {
  const plan = {
    ...FIXTURE_PLAN,
    primary_spec_id: "spec-fixture",
    sections: [{ ...FIXTURE_SECTION, id: "plan-sec", title: "Plan section" }],
    tasks: [{ ...FIXTURE_TASK, section_ids: ["plan-sec"] }],
  };
  const rows = buildPlanListRows(plan, { tabs });
  assert(rows.some((r) => r.taskId === FIXTURE_TASK.id && r.sectionTitle === "Plan section"), "plan section carried on task row");
  assert(!rows.some((r) => r.kind === "plan-section" && r.title === "Plan section"), "no plan section header");
  assert(!rows.some((r) => (r.kind as string) === "spec-section"), "no spec sections in list");
  assert(!rows.some((r) => (r.kind as string) === "spec-root"), "no spec root in list");
});

test("forest flattens multiple plans keyed by distinct plan ids", () => {
  const plans = bandedSeedPlans().slice(0, 2);
  const rows = buildPlanListForest(plans, { tabs });
  const taskPlanIds = new Set(
    rows.filter((r) => r.kind === "task" || r.kind === "subtask").map((r) => r.planId),
  );
  assert(taskPlanIds.size === 2, "tasks from two distinct plans");
  assert(!rows.some((r) => (r.kind as string) === "plan-root"), "no plan-root bands");
});

test("section rows carry doneCount and allTasksDone for fully done sections", () => {
  const plan = {
    ...FIXTURE_PLAN,
    sections: [{ ...FIXTURE_SECTION, id: "sec-done", title: "Done section" }],
    tasks: [
      { ...FIXTURE_TASK, id: "t1", status: "done" as const, section_ids: ["sec-done"] },
      { ...FIXTURE_TASK, id: "t2", status: "done" as const, name: "Second", section_ids: ["sec-done"] },
    ],
  };
  const section = buildPlanListRows(plan, { tabs }).find((r) => r.title === "Done");
  assert(section?.doneCount === 2, "done count");
  assert(section?.allTasksDone === true, "all done flag");
  assert(defaultSectionExpanded(section!) === false, "done-only section defaults collapsed");
});

test("active status group defaults expanded", () => {
  const plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [{ ...FIXTURE_TASK, status: "in_progress" as const }],
  };
  const section = buildPlanListRows(plan, { tabs }).find((r) => r.title === "In progress + review");
  assert(section && defaultSectionExpanded(section) === true, "active section expanded by default");
});

test("buildVisiblePlanListRows collapses done-only sections to rollup", () => {
  const plan = {
    ...FIXTURE_PLAN,
    sections: [{ ...FIXTURE_SECTION, id: "sec-done", title: "Done section" }],
    tasks: [
      { ...FIXTURE_TASK, id: "t1", status: "done" as const, section_ids: ["sec-done"] },
      { ...FIXTURE_TASK, id: "t2", status: "done" as const, name: "Second", section_ids: ["sec-done"] },
    ],
  };
  const rows = buildPlanListRows(plan, { tabs });
  const visible = buildVisiblePlanListRows(rows, {
    statusFilter: "all",
    textQuery: "",
    expansionOverrides: new Map(),
  });
  assert(visible.some((v) => v.type === "done-rollup"), "rollup row");
  assert(!visible.some((v) => v.type === "row" && v.row.taskId === "t1"), "done tasks hidden when collapsed");
});

test("done filter expands collapsed done group", () => {
  const plan = {
    ...FIXTURE_PLAN,
    tasks: [
      { ...FIXTURE_TASK, id: "t1", status: "done" as const },
      { ...FIXTURE_TASK, id: "t2", status: "done" as const, name: "Second" },
    ],
  };
  const rows = buildPlanListRows(plan, { tabs });
  const visible = buildVisiblePlanListRows(rows, {
    statusFilter: "done",
    textQuery: "",
    expansionOverrides: new Map(),
  });
  assert(!visible.some((v) => v.type === "done-rollup"), "no rollup under explicit done filter");
  assert(visible.some((v) => v.type === "row" && v.row.taskId === "t1"), "done task visible");
});

test("status filter hides non-matching tasks", () => {
  const plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [
      { ...FIXTURE_TASK, id: "t-active", status: "in_progress" as const },
      { ...FIXTURE_TASK, id: "t-done", status: "done" as const, name: "Finished" },
    ],
  };
  const rows = buildPlanListRows(plan, { tabs });
  const filter = (status: PlanListStatusFilter) =>
    buildVisiblePlanListRows(rows, { statusFilter: status, textQuery: "", expansionOverrides: new Map() })
      .filter((v) => v.type === "row" && v.row.taskId)
      .map((v) => (v as { type: "row"; row: { taskId?: string } }).row.taskId);
  assert(filter("done").includes("t-done"), "done filter keeps done task");
  assert(!filter("done").includes("t-active"), "done filter drops active task");
  assert(filter("active").includes("t-active"), "active filter keeps in-progress");
});

test("text filter matches task titles", () => {
  const plan = {
    ...FIXTURE_PLAN,
    tasks: [
      { ...FIXTURE_TASK, id: "t1", name: "Alpha task" },
      { ...FIXTURE_TASK, id: "t2", name: "Beta task" },
    ],
  };
  const rows = buildPlanListRows(plan, { tabs });
  const visible = buildVisiblePlanListRows(rows, {
    statusFilter: "all",
    textQuery: "beta",
    expansionOverrides: new Map(),
  });
  const taskIds = visible
    .filter((v) => v.type === "row" && v.row.taskId)
    .map((v) => (v as { type: "row"; row: { taskId?: string } }).row.taskId);
  assert(taskIds.includes("t2"), "beta visible");
  assert(!taskIds.includes("t1"), "alpha hidden");
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
console.log(`\n✓ ${passed}/${tests.length} buildPlanListTree tests passed`);
