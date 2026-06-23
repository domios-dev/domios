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

function task(id: string, sectionIds: string[], milestones: Milestone[] = []): Task {
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
  // s1a row sits below s1 row by exactly one row height.
  const a1a = frame.anchors.get("s1a")!;
  ok("subsection anchor one row below parent", Math.abs(a1a.y - a1.y - DEFAULT_CANVAS_SETTINGS.planRowHeight) < 0.5);
  // the section→task edge originates exactly at the section row anchor.
  const edge = frame.edges.find((e) => e.kind === "section-task" && e.to === "t1");
  ok("section-task edge starts at the row anchor", !!edge?.fromPoint && Math.abs(edge.fromPoint.x - a1.x) < 0.5 && Math.abs(edge.fromPoint.y - a1.y) < 0.5);

  // t1's vertical center is anchored to its primary covered row (s1) → level edge.
  const t1 = frame.nodes.find((n) => n.id === "t1")!;
  const t1Center = t1.box.y + t1.box.height / 2;
  ok("task center anchored to its primary covered-row Y (level edge)", Math.abs(t1Center - a1.y) < 1.5);
}

// ── spec node + plan-spec edge ────────────────────────────────────
console.log("\nspec node:");
{
  const layout = layoutWorkspace(samplePlans());
  const frame = layout.frames.find((f) => f.planId === "p1")!;
  const spec = frame.nodes.find((n) => n.kind === "spec");
  ok("spec node emitted for a plan with sections", !!spec);
  ok("plan-spec edge present", frame.edges.some((e) => e.kind === "plan-spec"));
  ok("spec sits left of plan node", spec!.box.x < frame.nodes.find((n) => n.kind === "plan")!.box.x);

  // Spec and the plan+tasks block are vertically centered against each other.
  const blockBoxes = frame.nodes.filter((n) => ["plan", "task", "milestone", "cluster"].includes(n.kind)).map((n) => n.box);
  const blockCenter = (Math.min(...blockBoxes.map((b) => b.y)) + Math.max(...blockBoxes.map((b) => b.y + b.height))) / 2;
  const specCenter = spec!.box.y + spec!.box.height / 2;
  ok("spec ↕centered against the plan+tasks block", Math.abs(specCenter - blockCenter) < 2);
}

// ── standalone / inbox plan (no sections) ─────────────────────────
console.log("\nstandalone plan:");
{
  const inbox = plan("inbox", "orchA", 10, [], [task("ti", [])]);
  const layout = layoutWorkspace([inbox]);
  const frame = layout.frames[0];
  ok("no spec node without sections", !frame.nodes.some((n) => n.kind === "spec"));
  ok("plan node still present", frame.nodes.some((n) => n.kind === "plan"));
  ok("task node present", frame.nodes.some((n) => n.kind === "task"));
  ok("no anchors without sections", frame.anchors.size === 0);
}

// ── milestone bubble cluster ──────────────────────────────────────
console.log("\nbubble cluster:");
{
  const layout = layoutWorkspace(samplePlans());
  const frame = layout.frames.find((f) => f.planId === "p1")!;
  const bubbles = frame.nodes.filter((n) => n.kind === "milestone");
  ok("all 4 milestones placed as bubbles", bubbles.length === 4);
  ok("task-cluster edge present", frame.edges.some((e) => e.kind === "task-cluster"));
  ok("milestone-chain edges present", frame.edges.filter((e) => e.kind === "milestone-chain").length === 2);

  // The cluster ABUTS its task (≤ one gutter away), never floats far right, and
  // wraps to ≤2 columns so it can't stream off-screen.
  const t1 = frame.nodes.find((n) => n.id === "t1")!;
  const t1Bubbles = [bubbles.find((b) => b.id === "m1")!, bubbles.find((b) => b.id === "m2")!, bubbles.find((b) => b.id === "m3")!];
  const t1Right = t1.box.x + t1.box.width;
  const nearest = Math.min(...t1Bubbles.map((b) => b.box.x));
  ok("cluster abuts the task (≤ ~2 gutters)", nearest - t1Right > 0 && nearest - t1Right < 100);
  const span = Math.max(...t1Bubbles.map((b) => b.box.x + b.box.width)) - nearest;
  ok("cluster wraps to ≤2 columns (compact width)", span <= 2 * (DEFAULT_CANVAS_SETTINGS.bubbleRadius * 2) + DEFAULT_CANVAS_SETTINGS.bubbleGapX + 1);
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
