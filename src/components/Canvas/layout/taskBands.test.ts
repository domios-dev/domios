// Run: npx tsx src/components/Canvas/layout/taskBands.test.ts

import type { Plan, Section, Task } from "../types";
import { DEFAULT_CANVAS_SETTINGS as S } from "../settings";
import type { CanvasViewMode } from "../cards/types";
import { layoutWorkspace, boxesOverlap, type NodeModeResolver } from "./index";

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const canvas = () => ({ x: null, y: null, collapsed: false });
function section(id: string): Section {
  return { id, title: id, body: "spec body for " + id, subsections: [], canvas: canvas() };
}
function task(id: string, sectionIds: string[]): Task {
  return {
    id, name: id, type: "implement", model: null, model_rationale: null,
    status: "pending", status_label: null, section_ids: sectionIds, body: "",
    intent: null, assigned_pane_id: null, peer: null, parent_task_id: null,
    canvas: canvas(), milestones: [],
  };
}

// Dogfood scale: 24 sections, 38 tasks. Tasks concentrate on ~8 sections (the
// realistic shape — most coverage piles on a few sections) so banding wraps them
// into columns; plus a few standalone. This is §20.3's "19 tasks over ~8 bands".
function bigPlan(): Plan {
  const sections = Array.from({ length: 24 }, (_, i) => section(`s${i}`));
  const tasks: Task[] = [];
  for (let i = 0; i < 34; i++) tasks.push(task(`t${i}`, [`s${i % 8}`]));
  for (let i = 0; i < 4; i++) tasks.push(task(`u${i}`, [])); // standalone
  return {
    id: "big", title: "big", sections, status: "active",
    owner: { pane_id: "o", session_id: "o", title: "o" }, project_path: "/p",
    created_at: 0, updated_at: 1, milestone_kinds: [], loops: [], elements: [],
    tasks, canvas: { viewport: null }, owner_history: [],
  };
}

const frame = layoutWorkspace([bigPlan()]).frames[0];
const taskNodes = frame.nodes.filter((n) => n.kind === "task");
ok("all 38 tasks placed", taskNodes.length === 38);

// Bounded width: tasks occupy at most maxTaskCols distinct column X positions.
const colXs = new Set(taskNodes.map((n) => Math.round(n.box.x)));
ok(`tasks use ≤ maxTaskCols (${S.maxTaskCols}) columns, got ${colXs.size}`, colXs.size <= S.maxTaskCols);

// Horizontal sprawl hard-capped (planRight + maxCols*pitch).
const taskRight = Math.max(...taskNodes.map((n) => n.box.x + n.box.width));
const taskLeft = Math.min(...taskNodes.map((n) => n.box.x));
ok("task grid width within maxCols*colPitch", taskRight - taskLeft <= S.maxTaskCols * (S.taskWidth + S.taskColGap));

// The портянka is killed: 38 tasks tall would be ~38*taskMinHeight stacked; banding
// must be a small fraction of that (a few hundred px), since cols absorb the stack.
const taskTop = Math.min(...taskNodes.map((n) => n.box.y));
const taskBottom = Math.max(...taskNodes.map((n) => n.box.y + n.box.height));
const bandedH = taskBottom - taskTop;
// The real портянka: 38 Document cells (taskDocH) stacked one-per-row with gaps.
// Banding into <=3 cols must cut that by well over a third.
const singleColH = 38 * (S.taskDocH + S.taskGapY);
ok(`banded height (${Math.round(bandedH)}) far below single-column (${singleColH})`, bandedH < singleColH * 0.6);

// No task node overlaps another (top-down non-overlap sweep).
function noTaskOverlap(nodes = taskNodes): boolean {
  for (let i = 0; i < nodes.length; i++)
    for (let j = i + 1; j < nodes.length; j++)
      if (boxesOverlap(nodes[i].box, nodes[j].box)) return false;
  return true;
}
ok("no task-node overlap after banded sweep", noTaskOverlap());

// ── Height-bound regression (the ~20000px km-strip bug) ───────────
// Real dogfood shape: 25 sections, 23 top-level tasks spread ~1/section, 22
// subtasks under parents, 1 standalone — most bands hold a single (super)cell, so
// a sequential stack would be a km strip. Skyline packing must keep it bounded.
function dogfoodPlan(): Plan {
  const sections = Array.from({ length: 25 }, (_, i) => section(`s${i}`));
  const tasks: Task[] = [];
  for (let i = 0; i < 23; i++) tasks.push(task(`t${i}`, [`s${i}`])); // ~1 per section
  for (let i = 0; i < 22; i++) {
    const sub = task(`sub${i}`, []);
    sub.parent_task_id = `t${i % 23}`;
    tasks.push(sub);
  }
  tasks.push(task("u0", [])); // standalone
  return {
    id: "dog", title: "dog", sections, status: "active",
    owner: { pane_id: "o", session_id: "o", title: "o" }, project_path: "/p",
    created_at: 0, updated_at: 1, milestone_kinds: [], loops: [], elements: [],
    tasks, canvas: { viewport: null }, owner_history: [],
  };
}

const fixed = (m: CanvasViewMode): NodeModeResolver => () => m;
for (const [mode, bound] of [["tile", 2600] as const, ["card", 4200] as const]) {
  const lo = layoutWorkspace([dogfoodPlan()], S, fixed(mode));
  ok(`${mode} world height bounded (${Math.round(lo.bounds.height)} < ${bound}) — no km strip`, lo.bounds.height < bound);
  const tn = lo.frames[0].nodes.filter((n) => n.kind === "task");
  ok(`${mode}: no task overlap at dogfood scale`, noTaskOverlap(tn));
  // Top-level cells use ≤ maxTaskCols columns (children may indent one more).
  const topXs = new Set(lo.frames[0].nodes.filter((n) => n.kind === "task" && !n.id.startsWith("sub")).map((n) => Math.round(n.box.x)));
  ok(`${mode}: top-level tasks within ≤ maxTaskCols columns`, topXs.size <= S.maxTaskCols);
}

// Determinism: same input → identical task boxes.
const frame2 = layoutWorkspace([bigPlan()]).frames[0];
const t2 = frame2.nodes.filter((n) => n.kind === "task");
const same = taskNodes.every((n, i) => n.id === t2[i].id && n.box.x === t2[i].box.x && n.box.y === t2[i].box.y);
ok("banded layout is deterministic", same);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
