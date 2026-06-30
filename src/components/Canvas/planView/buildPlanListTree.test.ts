// Run: npx tsx src/components/Canvas/planView/buildPlanListTree.test.ts

import type { Pane } from "../../../lib/types";
import { FIXTURE_MILESTONE, FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { bandedSeedPlans } from "../layout/fixtures";
import { buildPlanListForest, buildPlanListRows } from "./buildPlanListTree";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const panes: Record<string, Pane> = {
  "pane-42": {
    id: "pane-42",
    pty_id: null,
    pane_type: "Terminal",
    title: "Puccini",
    cwd: "/tmp",
    agent_type: "codex",
  },
};

test("orders spec root, spec sections, plan root, plan sections, tasks, subtasks", () => {
  const [plan] = bandedSeedPlans();
  const rows = buildPlanListRows(plan, { specSections: plan.sections, panes });
  const kinds = rows.map((r) => r.kind);
  const specRoot = kinds.indexOf("spec-root");
  const planRoot = kinds.indexOf("plan-root");
  const firstTask = kinds.indexOf("task");
  const firstSubtask = kinds.indexOf("subtask");
  assert(specRoot >= 0, "spec root present");
  assert(planRoot > specRoot, "plan after spec");
  assert(firstTask > planRoot, "tasks after plan root");
  assert(firstSubtask > firstTask, "subtasks after parent task");
});

test("nests subtasks under parent task in section", () => {
  const [plan] = bandedSeedPlans();
  const rows = buildPlanListRows(plan, { panes });
  const parentIdx = rows.findIndex((r) => r.taskId === "t_parent");
  const subAIdx = rows.findIndex((r) => r.taskId === "t_sub_a");
  assert(parentIdx >= 0 && subAIdx > parentIdx, "subtask follows parent");
  assert(rows[subAIdx].kind === "subtask", "child row kind");
  assert(rows[subAIdx].depth > rows[parentIdx].depth, "subtask indented deeper");
});

test("lists inbox tasks under their section", () => {
  const [plan] = bandedSeedPlans();
  const rows = buildPlanListRows(plan, { panes });
  const inboxSection = rows.find((r) => r.kind === "plan-section" && r.title === "sec_inbox");
  assert(inboxSection, "inbox section row");
  const inbox = rows.find((r) => r.taskId === "t_inbox");
  assert(inbox && inbox.kind === "task", "inbox task listed");
});

test("unsectioned bucket for tasks without section_ids", () => {
  const plan = {
    ...FIXTURE_PLAN,
    tasks: [{ ...FIXTURE_TASK, section_ids: [] }],
  };
  const rows = buildPlanListRows(plan, { panes });
  assert(rows.some((r) => r.kind === "unsectioned"), "unsectioned header");
});

test("surfaces pane label and milestone on active assigned task", () => {
  const plan = {
    ...FIXTURE_PLAN,
    heartbeat: { current_focus: "Ship auth", active_lanes: ["task:task-dante"] },
    tasks: [
      {
        ...FIXTURE_TASK,
        assigned_pane_id: "pane-42",
        milestones: [
          {
            ...FIXTURE_MILESTONE,
            from: { pane_id: "pane-42", session_id: null, title: null },
            kind: "status_update",
          },
        ],
      },
    ],
  };
  const taskRow = buildPlanListRows(plan, { panes }).find((r) => r.taskId === FIXTURE_TASK.id);
  assert(taskRow?.paneLabel === "Puccini", "pane title");
  assert(!taskRow?.historicalExecutorSummary, "active task has no historical summary");
  assert(taskRow?.milestoneKind === "status update", "milestone kind label");
});

test("done task surfaces historical executor instead of active pane label", () => {
  const plan = {
    ...FIXTURE_PLAN,
    tasks: [
      {
        ...FIXTURE_TASK,
        status: "done" as const,
        assigned_pane_id: "pane-42",
        taken_by_role: "backend-dev",
        model: "gpt-5.5",
        lane: { pane_id: "pane-42", branch: "feat/history" },
      },
    ],
  };
  const taskRow = buildPlanListRows(plan, { panes }).find((r) => r.taskId === FIXTURE_TASK.id);
  assert(!taskRow?.paneLabel, "done task has no active pane label");
  assert(taskRow?.historicalExecutorSummary?.includes("Puccini"), "historical summary shown");
});

test("spec sections come from fetched spec tree, plan sections from plan.sections", () => {
  const plan = {
    ...FIXTURE_PLAN,
    primary_spec_id: "spec-fixture",
    sections: [{ ...FIXTURE_SECTION, id: "plan-sec", title: "Plan section" }],
  };
  const specSections = [{ ...FIXTURE_SECTION, id: "spec-sec", title: "Spec section" }];
  const rows = buildPlanListRows(plan, { specSections, panes });
  assert(rows.some((r) => r.kind === "spec-section" && r.title === "Spec section"), "spec section from spec tree");
  assert(rows.some((r) => r.kind === "plan-section" && r.title === "Plan section"), "plan section from plan tree");
  assert(!rows.some((r) => r.kind === "spec-section" && r.title === "Plan section"), "no cross-contamination");
});

test("forest flattens multiple plans with distinct plan roots", () => {
  const plans = bandedSeedPlans().slice(0, 2);
  const rows = buildPlanListForest(plans, { panes });
  const planRoots = rows.filter((r) => r.kind === "plan-root");
  assert(planRoots.length === 2, "two plan roots");
  assert(new Set(planRoots.map((r) => r.planId)).size === 2, "distinct plan ids");
});

test("section rows carry task counts from sectionRows", () => {
  const [plan] = bandedSeedPlans();
  const rows = buildPlanListRows(plan, { panes });
  const layoutRow = rows.find((r) => r.kind === "plan-section" && r.title === "sec_layout");
  assert(layoutRow && (layoutRow.taskCount ?? 0) > 0, "layout section has tasks");
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
