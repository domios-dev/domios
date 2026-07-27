// Run: npx tsx src/components/Canvas/paneCurrentTask.test.ts

import { resolvePaneCurrentTask, resolvePlanExclusivePaneTasks } from "./paneCurrentTask";
import type { Plan, Task } from "./types";

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

const emptyCanvas = { x: null, y: null, collapsed: false };

function task(over: Partial<Task>): Task {
  return {
    id: "t",
    name: "Task",
    type: "implement",
    model: null,
    model_rationale: null,
    status: "pending",
    status_label: null,
    section_ids: [],
    body: "",
    intent: null,
    parent_task_id: null,
    assigned_pane_id: null,
    peer: null,
    canvas: emptyCanvas,
    milestones: [],
    ...over,
  };
}

function plan(tasks: Task[], over: Partial<Plan> = {}): Plan {
  return {
    id: "plan-1",
    title: "Plan",
    sections: [],
    status: "active",
    owner: { pane_id: null, session_id: null, title: null },
    project_path: "/p",
    created_at: 0,
    updated_at: 0,
    milestone_kinds: [],
    loops: [],
    elements: [],
    tasks,
    canvas: { viewport: null },
    owner_history: [],
    ...over,
  };
}

ok("null Tab → null", resolvePaneCurrentTask([plan([task({ assigned_pane_id: "p1" })])], null) === null);
ok("no match → null", resolvePaneCurrentTask([plan([task({ assigned_pane_id: "p1" })])], "pX") === null);

const byAssign = resolvePaneCurrentTask(
  [plan([task({ id: "a", assigned_pane_id: "p1" })])],
  "p1",
);
ok("assigned_pane_id match", byAssign?.task.id === "a");

const byLane = resolvePaneCurrentTask(
  [plan([task({ id: "b", assigned_pane_id: null, lane: { pane_id: "p2", branch: "feat/x" } })])],
  "p2",
);
ok("lane.pane_id match", byLane?.task.id === "b");
ok("branch surfaced from lane", byLane?.branch === "feat/x");

const live = resolvePaneCurrentTask(
  [plan([
    task({ id: "done", assigned_pane_id: "p1", status: "done", status_changed_at: 999 }),
    task({ id: "wip", assigned_pane_id: "p1", status: "in_progress", status_changed_at: 1 }),
  ])],
  "p1",
);
ok("in_progress beats done regardless of freshness", live?.task.id === "wip");

const closedOnly = resolvePaneCurrentTask(
  [plan([
    task({ id: "done", assigned_pane_id: "p1", lane: { pane_id: "p1" }, status: "done", status_changed_at: 999 }),
    task({ id: "cancelled", assigned_pane_id: "p1", status: "cancelled", status_changed_at: 1000 }),
  ])],
  "p1",
);
ok("done/cancelled bindings do not create current work", closedOnly === null);

const fresh = resolvePaneCurrentTask(
  [plan([
    task({ id: "old", assigned_pane_id: "p1", status: "pending", updated_at: 10 }),
    task({ id: "new", assigned_pane_id: "p1", status: "pending", updated_at: 20 }),
  ])],
  "p1",
);
ok("same status → freshest wins", fresh?.task.id === "new");

const archived = resolvePaneCurrentTask(
  [plan([task({ id: "z", assigned_pane_id: "p1" })], { status: "archived" })],
  "p1",
);
ok("archived plan ignored", archived === null);

const exclusive = resolvePlanExclusivePaneTasks(
  plan([
    task({ id: "wip", assigned_pane_id: "p1", status: "in_progress", updated_at: 20 }),
    task({ id: "stale", assigned_pane_id: "p1", status: "pending", updated_at: 10 }),
  ]),
);
ok("exclusive map picks in_progress task", exclusive.get("p1") === "wip");

const closedExclusive = resolvePlanExclusivePaneTasks(
  plan([task({ id: "done", assigned_pane_id: "p1", lane: { pane_id: "p1" }, status: "done" })]),
);
ok("exclusive map ignores terminal stale bindings", !closedExclusive.has("p1"));

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
