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
  return { id, title: id, body: "", subsections: subs, canvas: canvas() };
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
  ok("identical positions", JSON.stringify(a.frames) === JSON.stringify(b.frames));
  ok("bounds identical", JSON.stringify(a.bounds) === JSON.stringify(b.bounds));
}

// ── no frame overlap ──────────────────────────────────────────────
console.log("\nno overlap:");
{
  const layout = layoutWorkspace(samplePlans());
  let overlap = false;
  for (let i = 0; i < layout.frames.length; i++) {
    for (let j = i + 1; j < layout.frames.length; j++) {
      if (boxesOverlap(layout.frames[i].box, layout.frames[j].box)) overlap = true;
    }
  }
  ok("no two frames overlap", !overlap);
}

// ── owner clustering ──────────────────────────────────────────────
console.log("\nclustering:");
{
  const layout = layoutWorkspace(samplePlans());
  const byPlan = new Map(layout.frames.map((f) => [f.planId, f]));
  // orchA cluster (p1,p2) shares a y-band distinct from orchB (p3).
  const aTop = Math.min(byPlan.get("p1")!.box.y, byPlan.get("p2")!.box.y);
  const bTop = byPlan.get("p3")!.box.y;
  ok("orchA cluster above orchB", aTop < bTop);
  // within orchA, p2 (updatedAt 50) sorts before p1 (updatedAt 100).
  ok("intra-cluster sorted by updatedAt", byPlan.get("p2")!.box.x < byPlan.get("p1")!.box.x);
}

// ── focus selection ───────────────────────────────────────────────
console.log("\nfocus:");
{
  const layout = layoutWorkspace(samplePlans());
  const frame = layout.frames.find((f) => f.planId === "p1")!;
  const center = boxCenter(frame.box);
  const focus = focusAt(layout, center, DEFAULT_CANVAS_SETTINGS);
  ok("center → correct plan", focus.planId === "p1");
  ok("focus returns in-view ids", focus.sectionIds.length + focus.taskIds.length + focus.milestoneIds.length > 0);

  // a point far outside everything still resolves to nearest plan, never crashes.
  const far = focusAt(layout, { x: 999999, y: 999999 }, DEFAULT_CANVAS_SETTINGS);
  ok("far point → some plan", far.planId !== null);

  const empty = focusAt({ frames: [], nodeIndex: new Map(), bounds: { x: 0, y: 0, width: 0, height: 0 } }, { x: 0, y: 0 }, DEFAULT_CANVAS_SETTINGS);
  ok("empty layout → null plan", empty.planId === null);
}

// ── chain width-bounded ───────────────────────────────────────────
console.log("\nchain bounds:");
{
  const ms: Milestone[] = [];
  for (let i = 0; i < 30; i++) ms.push(milestone(`mm${i}`, "status_update", i));
  const big = plan("pb", "o", 0, [section("s")], [task("tb", ["s"], ms)]);
  const layout = layoutWorkspace([big]);
  const frame = layout.frames[0];
  const msNodes = frame.nodes.filter((n) => n.kind === "milestone");
  ok("all 30 milestones placed", msNodes.length === 30);
  const minX = Math.min(...msNodes.map((n) => n.box.x));
  const maxX = Math.max(...msNodes.map((n) => n.box.x + n.box.width));
  ok("chain width bounded (folds vertically)", maxX - minX <= DEFAULT_CANVAS_SETTINGS.chainWrapWidth + 1);
  const maxY = Math.max(...msNodes.map((n) => n.box.y + n.box.height));
  const minY = Math.min(...msNodes.map((n) => n.box.y));
  ok("chain uses vertical space", maxY - minY > 0);
}

// ── manual clamp ──────────────────────────────────────────────────
console.log("\nmanual clamp:");
{
  const manualSection = section("s-manual");
  manualSection.canvas = { x: 99999, y: 99999, collapsed: false };
  const p = plan("pclamp", "o", 0, [manualSection], [task("t1", ["s-manual"])]);
  const layout = layoutWorkspace([p]);
  const frame = layout.frames[0];
  const secNode = frame.nodes.find((n) => n.id === "s-manual")!;
  const pad = DEFAULT_CANVAS_SETTINGS.framePadding;
  ok("manual x clamped inside frame", secNode.box.x < frame.box.width - pad);
  ok("manual y clamped inside frame", secNode.box.y < frame.box.height - pad);
  ok("manual x respects padding", secNode.box.x >= pad);
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
  const all = visibleNodeIds(layout, { x: 0, y: 0, scale: 0.01 }, { width: 20000, height: 20000 }, 0);
  ok("tight viewport culls", tight.length < all.length);
  ok("wide viewport sees all nodes", all.length === [...layout.nodeIndex.keys()].length);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
