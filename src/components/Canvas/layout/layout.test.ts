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
  return { tab_id: id, session_id: id, title: id };
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
    assigned_tab_id: null,
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
  p1.primary_spec_id = "spec-p1";
  const p2 = plan("p2", "orchA", 50, [section("s3")], [task("t3", ["s3"])]);
  const p3 = plan("p3", "orchB", 75, [section("s4")], [task("t4", ["s4"])]);
  return [p1, p2, p3];
}

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
  // sits one rail-span below s1 (worlds-v3 §1: plan rows use plan height).
  const a1a = frame.anchors.get("s1a")!;
  const railSpan = (DEFAULT_CANVAS_SETTINGS.planHeight - DEFAULT_CANVAS_SETTINGS.planHeaderHeight) / 3;
  ok("rail outputs evenly distributed (one span apart)", Math.abs(a1a.y - a1.y - railSpan) < 0.5);
  // the section→task edge originates exactly at the section row anchor.
  const edge = frame.edges.find((e) => e.kind === "section-task" && e.to === "t1");
  ok("section-task edge starts at the row anchor", !!edge?.fromPoint && Math.abs(edge.fromPoint.x - a1.x) < 0.5 && Math.abs(edge.fromPoint.y - a1.y) < 0.5);

  // worlds-v3 §2 circle-pack disk: t1 sits in the task disk RIGHT of the plan;
  // the section→task edge keeps the WHAT→HOW→task mapping.
  const t1Node = frame.nodes.find((n) => n.id === "t1")!;
  ok("task sits in the disk right of the plan", t1Node.box.x >= planNode.box.x + planNode.box.width - 1);
}

console.log("\nspec + plan nodes:");
{
  const layout = layoutWorkspace(samplePlans());
  const frame = layout.frames.find((f) => f.planId === "p1")!;
  const planNodes = frame.nodes.filter((n) => n.kind === "plan");
  const specNodes = frame.nodes.filter((n) => n.kind === "spec");
  ok("exactly one plan node + one spec node", planNodes.length === 1 && specNodes.length === 1);
  // "From spec" chips, not a wire fan).
  ok("exactly one spec→plan link", frame.edges.filter((e) => e.kind === "spec-plan").length === 1);
  // spec a smaller doc node; heights now differ.
  ok("plan node fixed height (planHeight)", planNodes[0].box.height === DEFAULT_CANVAS_SETTINGS.planHeight);
  ok("spec node fixed height + width", specNodes[0].box.height === DEFAULT_CANVAS_SETTINGS.specNodeH && specNodes[0].box.width === DEFAULT_CANVAS_SETTINGS.specNodeW);
  ok("plan bigger than spec in both axes", planNodes[0].box.width > specNodes[0].box.width && planNodes[0].box.height > specNodes[0].box.height);
  // Column order: SPEC | PLAN | tasks.
  ok("spec sits left of plan", specNodes[0].box.x + specNodes[0].box.width <= planNodes[0].box.x);
  const planRight = planNodes[0].box.x + planNodes[0].box.width;
  const tasks = frame.nodes.filter((n) => n.kind === "task");
  ok("tasks sit right of the plan node", tasks.every((t) => t.box.x >= planRight - 1));
}

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

console.log("\ninterior steps:");
{
  const layout = layoutWorkspace(samplePlans());
  const frame = layout.frames.find((f) => f.planId === "p1")!;
  // Document task card as step cards (Thread & Coil interior). No cluster chrome.
  ok("no separate milestone nodes (steps are interior)", !frame.nodes.some((n) => n.kind === "milestone"));
  ok("no cluster nodes / task-cluster edges", !frame.nodes.some((n) => n.kind === "cluster") && !frame.edges.some((e) => e.kind === "task-cluster"));
  // v3 zone grid: uniform cell size per task (volume does not change grid slot).
  const S = DEFAULT_CANVAS_SETTINGS;
  const withSteps = frame.nodes.find((n) => n.id === "t1")!;
  const noSteps = frame.nodes.find((n) => n.id === "t2")!;
  ok("uniform grid cell width", withSteps.box.width === S.zoneTaskW && noSteps.box.width === S.zoneTaskW);
  ok("uniform grid cell height", withSteps.box.height === S.zoneTaskH && noSteps.box.height === S.zoneTaskH);
}

console.log("\nsubtasks:");
{
  const parent = task("tp", ["s"]);
  const c1 = task("tc1", ["s"], [], "tp");
  const c2 = task("tc2", ["s"], [], "tp");
  const psub = plan("psub", "o", 0, [section("s")], [parent, c1, c2]);
  const layout = layoutWorkspace([psub]);
  const frame = layout.frames[0];
  const byId = new Map(frame.nodes.map((n) => [n.id, n]));
  ok("parent + 2 children all laid out as solid task nodes", ["tp", "tc1", "tc2"].every((id) => byId.get(id)?.kind === "task"));
  ok("no subtask-group container", !frame.nodes.some((n) => n.kind === "subtask-group"));
  ok("no subtask stem edges", frame.edges.filter((e) => e.kind === "subtask").length === 0);
  const tp = byId.get("tp")!;
  const tc1 = byId.get("tc1")!;
  const tc2 = byId.get("tc2")!;
  ok("subtasks share uniform grid cells with parent", tp.box.width === tc1.box.width && tp.box.height === tc1.box.height && tc1.box.width === tc2.box.width);
  ok("subtasks placed in same section zone", frame.zones.some((z) => z.sectionId === "s" && z.box));
}

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

console.log("\nfiltering:");
{
  const plans = samplePlans();
  plans[0].status = "archived";
  const layout = layoutWorkspace(plans);
  ok("archived plan excluded", !layout.frames.some((f) => f.planId === "p1"));
}

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
