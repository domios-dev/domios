// Run: npx tsx src/components/Canvas/layout/layout.test.ts

import type { Plan, Section, Task, Milestone, OrchestratorRef } from "../types";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import { layoutWorkspace, focusAt, visibleNodeIds, boxesOverlap, boxCenter } from "./index";

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

function canvas() {
  return { x: null, y: null, collapsed: false };
}

function owner(id: string): OrchestratorRef {
  return { pane_id: id, session_id: id, title: id };
}

function section(id: string, subs: Section[] = []): Section {
  return { id, title: id, body: "body", subsections: subs, canvas: canvas() };
}

function milestone(id: string, kind: string, iteration: number | null = null, links: string[] = []): Milestone {
  return { id, from: owner("o"), kind, text: id, ts: 0, links, loop_id: null, iteration, canvas: canvas() };
}

function task(id: string, sectionIds: string[], milestones: Milestone[] = [], parentTaskId: string | null = null): Task {
  return {
    id,
    name: id,
    type: "implement",
    model: null,
    model_rationale: null,
    status: "pending",
    status_label: null,
    section_ids: sectionIds,
    body: "",
    intent: null,
    assigned_pane_id: null,
    peer: null,
    parent_task_id: parentTaskId,
    canvas: canvas(),
    milestones,
  };
}

function plan(id: string, ownerId: string, updatedAt: number, sections: Section[], tasks: Task[]): Plan {
  return {
    id,
    title: `plan-${id}`,
    sections,
    status: "active",
    owner: owner(ownerId),
    project_path: "/p",
    created_at: 0,
    updated_at: updatedAt,
    milestone_kinds: [
      { key: "status_update", label: "Status", color: null },
      { key: "testing", label: "Testing", color: null },
      { key: "review", label: "Review", color: null },
    ],
    loops: [],
    elements: [],
    tasks,
    canvas: { viewport: null },
    owner_history: [],
  };
}

function samplePlans(): Plan[] {
  const p1 = plan(
    "p1",
    "orchA",
    100,
    [section("s1", [section("s1a")]), section("s2")],
    [
      task("t1", ["s1"], [
        milestone("m1", "status_update", 0, ["m2"]),
        milestone("m2", "testing", 0, ["m3"]),
        milestone("m3", "review", 0),
      ]),
      task("t2", ["s2", "s1"], [milestone("m4", "status_update", 0)]),
    ],
  );
  const p2 = plan("p2", "orchA", 50, [section("s3")], [task("t3", ["s3"])]);
  const p3 = plan("p3", "orchB", 75, [section("s4")], [task("t4", ["s4"])]);
  return [p1, p2, p3];
}

// ── determinism ───────────────────────────────────────────────────
console.log("determinism:");
{
  const a = layoutWorkspace(samplePlans());
  const b = layoutWorkspace(samplePlans());
  ok("frame count stable", a.frames.length === b.frames.length && a.frames.length === 3);
  const stripAnchors = (l: ReturnType<typeof layoutWorkspace>) =>
    l.frames.map((f) => ({ ...f, anchors: [...f.anchors.entries()] }));
  ok("identical positions", JSON.stringify(stripAnchors(a)) === JSON.stringify(stripAnchors(b)));
  ok("bounds identical", JSON.stringify(a.bounds) === JSON.stringify(b.bounds));
}

// ── no overlap ────────────────────────────────────────────────────
console.log("\nno overlap:");
{
  const layout = layoutWorkspace(samplePlans());
  let frameOverlap = false;
  for (let i = 0; i < layout.frames.length; i++) {
    for (let j = i + 1; j < layout.frames.length; j++) {
      if (boxesOverlap(layout.frames[i].box, layout.frames[j].box)) frameOverlap = true;
    }
  }
  ok("no two frames overlap", !frameOverlap);

  // tasks/spec/plan/bubble nodes inside one frame never overlap each other. The
  // cluster background intentionally sits UNDER its bubbles, so exclude it.
  const frame = layout.frames.find((f) => f.planId === "p1")!;
  const solid = frame.nodes.filter((n) => n.kind !== "cluster");
  let nodeOverlap = false;
  for (let i = 0; i < solid.length; i++) {
    for (let j = i + 1; j < solid.length; j++) {
      if (boxesOverlap(solid[i].box, solid[j].box)) nodeOverlap = true;
    }
  }
  ok("no two nodes overlap within a frame", !nodeOverlap);

  let groupOverlap = false;
  for (let i = 0; i < layout.groups.length; i++) {
    for (let j = i + 1; j < layout.groups.length; j++) {
      if (boxesOverlap(layout.groups[i].box, layout.groups[j].box)) groupOverlap = true;
    }
  }
  ok("orchestrator groups do not overlap", !groupOverlap);
}

// ── owner grouping ────────────────────────────────────────────────
console.log("\ngrouping:");
{
  const layout = layoutWorkspace(samplePlans());
  ok("one group per distinct owner", layout.groups.length === 2);
  const byKey = new Map(layout.groups.map((g) => [g.title, g]));
  ok("orchA group above orchB", byKey.get("orchA")!.box.y < byKey.get("orchB")!.box.y);
  // every frame sits inside its owner group box.
  const groupByKey = new Map(layout.groups.map((g) => [g.ownerKey, g]));
  const inside = layout.frames.every((f) => {
    const g = groupByKey.get(f.ownerKey)!;
    return f.box.x >= g.box.x && f.box.y >= g.box.y;
  });
  ok("frames nested in their owner group", inside);
}

// ── section-row anchors ───────────────────────────────────────────
console.log("\nsection-row anchors:");
{
  const layout = layoutWorkspace(samplePlans());
  const frame = layout.frames.find((f) => f.planId === "p1")!;
  ok("anchor per flattened section (s1,s1a,s2)", frame.anchors.size === 3);
  const planNode = frame.nodes.find((n) => n.kind === "plan")!;
  const planRight = planNode.box.x + planNode.box.width;
  const a1 = frame.anchors.get("s1")!;
  ok("anchor on plan node right edge", Math.abs(a1.x - planRight) <= DEFAULT_CANVAS_SETTINGS.anchorInset + 1);
  ok("anchor y within plan node bounds", a1.y >= planNode.box.y && a1.y <= planNode.box.y + planNode.box.height);
  // Rail outputs are evenly distributed (§23.1): s1a sits one rail-span below s1.
  const a1a = frame.anchors.get("s1a")!;
  const railSpan = (DEFAULT_CANVAS_SETTINGS.sectionNodeH - DEFAULT_CANVAS_SETTINGS.planHeaderHeight) / 3;
  ok("rail outputs evenly distributed (one span apart)", Math.abs(a1a.y - a1.y - railSpan) < 0.5);
  // the section→task edge originates exactly at the section row anchor.
  const edge = frame.edges.find((e) => e.kind === "section-task" && e.to === "t1");
  ok("section-task edge starts at the row anchor", !!edge?.fromPoint && Math.abs(edge.fromPoint.x - a1.x) < 0.5 && Math.abs(edge.fromPoint.y - a1.y) < 0.5);

  // Banded sweep (§20.3): t1's CELL block hugs its primary covered row (s1) — the
  // row anchor falls inside the cell's vertical span (card + folded step cluster).
  const t1CellNodes = frame.nodes.filter((n) => n.id === "t1" || n.id === "t1::cluster" || ["m1", "m2", "m3"].includes(n.id));
  const cellTop = Math.min(...t1CellNodes.map((n) => n.box.y));
  const cellBottom = Math.max(...t1CellNodes.map((n) => n.box.y + n.box.height));
  ok("task cell hugs its primary covered-row Y (banded sweep)", cellTop <= a1.y + 1 && cellBottom >= a1.y - 1);
}

// ── SEPARATE spec + plan nodes (§24.3): three connected columns ──
console.log("\nspec + plan nodes:");
{
  const layout = layoutWorkspace(samplePlans());
  const frame = layout.frames.find((f) => f.planId === "p1")!;
  const planNodes = frame.nodes.filter((n) => n.kind === "plan");
  const specNodes = frame.nodes.filter((n) => n.kind === "spec");
  ok("exactly one plan node + one spec node", planNodes.length === 1 && specNodes.length === 1);
  // 1:1 spec↔plan edges (§24.3), one per flattened section.
  ok("one spec↔plan edge per section", frame.edges.filter((e) => e.kind === "spec-row").length === 3);
  // Spec + plan share the fixed sectionNode height/width (§24.1).
  ok("plan node fixed height", planNodes[0].box.height === DEFAULT_CANVAS_SETTINGS.sectionNodeH);
  ok("spec node fixed height + width", specNodes[0].box.height === DEFAULT_CANVAS_SETTINGS.sectionNodeH && specNodes[0].box.width === DEFAULT_CANVAS_SETTINGS.specNodeW);
  // Column order: SPEC | PLAN | tasks.
  ok("spec sits left of plan", specNodes[0].box.x + specNodes[0].box.width <= planNodes[0].box.x);
  const planRight = planNodes[0].box.x + planNodes[0].box.width;
  const tasks = frame.nodes.filter((n) => n.kind === "task");
  ok("tasks sit right of the plan node", tasks.every((t) => t.box.x >= planRight - 1));
}

// ── standalone / inbox plan (no sections) ─────────────────────────
console.log("\nstandalone plan:");
{
  const inbox = plan("inbox", "orchA", 10, [], [task("ti", [])]);
  const layout = layoutWorkspace([inbox]);
  const frame = layout.frames[0];
  ok("no spec cards without sections", !frame.nodes.some((n) => n.kind === "spec-section"));
  ok("plan node still present", frame.nodes.some((n) => n.kind === "plan"));
  ok("task node present", frame.nodes.some((n) => n.kind === "task"));
  ok("no anchors without sections", frame.anchors.size === 0);
}

// ── steps are INTERIOR to the task cell (§23.2 / §16.9) ───────────
console.log("\ninterior steps:");
{
  const layout = layoutWorkspace(samplePlans());
  const frame = layout.frames.find((f) => f.planId === "p1")!;
  // Milestones no longer become separate canvas nodes — they render inside the
  // Document task card as step cards (Thread & Coil interior). No cluster chrome.
  ok("no separate milestone nodes (steps are interior)", !frame.nodes.some((n) => n.kind === "milestone"));
  ok("no cluster nodes / task-cluster edges", !frame.nodes.some((n) => n.kind === "cluster") && !frame.edges.some((e) => e.kind === "task-cluster"));
  // §32.4 data-driven footprint: size ∝ work VOLUME (fixed across zoom, varies by
  // data), grows in BOTH axes bounded. t1 (3 steps) is taller than t2 (1 step);
  // width snaps to column slots in [taskW, taskMaxW]; height floored at aspectFloor.
  const S = DEFAULT_CANVAS_SETTINGS;
  const withSteps = frame.nodes.find((n) => n.id === "t1")!; // t1 has 3 milestones
  const noSteps = frame.nodes.find((n) => n.id === "t2")!; // t2 has 1 milestone
  ok("heavier task reserves a bigger footprint", withSteps.box.height > noSteps.box.height);
  ok("footprints within reserved bounds", [withSteps, noSteps].every((n) =>
    n.box.height >= S.taskMinH && n.box.height <= S.taskMaxH &&
    n.box.width >= S.taskW && n.box.width <= S.taskMaxW &&
    (n.box.width - S.taskW) % S.taskColStep === 0 &&
    n.box.height >= Math.round(S.aspectFloorRatio * n.box.width)));
}

// ── subtasks (parent_task_id) ─────────────────────────────────────
console.log("\nsubtasks:");
{
  const parent = task("tp", ["s"]);
  const c1 = task("tc1", [], [], "tp");
  const c2 = task("tc2", [], [], "tp");
  const psub = plan("psub", "o", 0, [section("s")], [parent, c1, c2]);
  const layout = layoutWorkspace([psub]);
  const frame = layout.frames[0];
  const byId = new Map(frame.nodes.map((n) => [n.id, n]));
  ok("parent + 2 children all laid out as tasks", ["tp", "tc1", "tc2"].every((id) => byId.get(id)?.kind === "task"));
  ok("subtask-group container present", frame.nodes.some((n) => n.kind === "subtask-group"));
  ok("two subtask stem edges", frame.edges.filter((e) => e.kind === "subtask").length === 2);
  const tp = byId.get("tp")!;
  const tc1 = byId.get("tc1")!;
  ok("child indented right of parent", tc1.box.x > tp.box.x);
  ok("child below the parent card", tc1.box.y >= tp.box.y + tp.box.height);
  // no section-task edge points at a child (children connect via the parent stem).
  ok("children are not top-level satellites", !frame.edges.some((e) => e.kind === "section-task" && (e.to === "tc1" || e.to === "tc2")));
}

// ── focus selection ───────────────────────────────────────────────
console.log("\nfocus:");
{
  const layout = layoutWorkspace(samplePlans());
  const frame = layout.frames.find((f) => f.planId === "p1")!;
  const center = boxCenter(frame.box);
  const focus = focusAt(layout, center, DEFAULT_CANVAS_SETTINGS);
  ok("center → correct plan", focus.planId === "p1");

  const far = focusAt(layout, { x: 999999, y: 999999 }, DEFAULT_CANVAS_SETTINGS);
  ok("far point → some plan", far.planId !== null);

  const empty = focusAt(
    { frames: [], nodeIndex: new Map(), groups: [], bounds: { x: 0, y: 0, width: 0, height: 0 } },
    { x: 0, y: 0 },
    DEFAULT_CANVAS_SETTINGS,
  );
  ok("empty layout → null plan", empty.planId === null);
}

// ── archived filtered ─────────────────────────────────────────────
console.log("\nfiltering:");
{
  const plans = samplePlans();
  plans[0].status = "archived";
  const layout = layoutWorkspace(plans);
  ok("archived plan excluded", !layout.frames.some((f) => f.planId === "p1"));
}

// ── virtualization ────────────────────────────────────────────────
console.log("\nvirtualization:");
{
  const layout = layoutWorkspace(samplePlans());
  const frame = layout.frames.find((f) => f.planId === "p1")!;
  const c = boxCenter(frame.box);
  const tight = visibleNodeIds(layout, { x: -c.x + 100, y: -c.y + 100, scale: 1 }, { width: 200, height: 200 }, 0);
  const all = visibleNodeIds(layout, { x: 0, y: 0, scale: 0.01 }, { width: 40000, height: 40000 }, 0);
  ok("tight viewport culls", tight.length < all.length);
  ok("wide viewport sees all nodes", all.length === [...layout.nodeIndex.keys()].length);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
