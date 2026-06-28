// Run: npx tsx src/components/Canvas/canvasContext.test.ts
//
// The composer's referent resolution: the open detail overlay node WINS; otherwise the
// scene node nearest the viewport centre; and the outgoing message carries explicit ids.

import { parseMarker } from "@codeterm/chat-core";
import { buildCanvasMessage, contextChipLabel, contextReference, resolveCanvasContext, toPlanContext } from "./canvasContext";
import { layoutWorkspace } from "./layout";
import { boxCenter } from "./layout/geometry";
import { bandedSeedPlans } from "./layout/fixtures";
import { boxContains } from "./layout";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const plans = bandedSeedPlans();
const layout = layoutWorkspace(plans);
const frame = layout.frames.find((f) => f.planId === "pl_banded_main")!;
const taskNode = frame.nodes.find((n) => n.id === "t_parent")!;
const farPoint = { x: -99999, y: -99999 }; // nowhere near any node

// (a) OVERLAY WINS: even with the centre pointing nowhere, the open node is the referent.
{
  const ctx = resolveCanvasContext({ overlay: { id: "t_parent", kind: "task", planId: "pl_banded_main" }, plans, layout, center: farPoint })!;
  ok("overlay node wins → task referent", ctx.kind === "task" && ctx.id === "t_parent");
  ok("overlay referent carries its planId", ctx.planId === "pl_banded_main");
  ok("overlay referent carries a title", ctx.title.length > 0);
}

// overlay spec → spec referent (id = the spec node id, planId routes the owner).
{
  const ctx = resolveCanvasContext({ overlay: { id: "spec:pl_banded_main", kind: "spec", planId: "pl_banded_main" }, plans, layout, center: farPoint })!;
  ok("overlay spec → spec referent", ctx.kind === "spec" && ctx.planId === "pl_banded_main");
}

// (b) RAYCAST CONTAINMENT: centre INSIDE a task's world-box → that exact task (not a
// fuzzy nearest). boxCenter is inside the box, so containment must hit it.
{
  const ctx = resolveCanvasContext({ overlay: null, plans, layout, center: boxCenter(taskNode.box) })!;
  ok("centre inside a task box → that task (containment)", ctx.kind === "task" && ctx.id === "t_parent");
  ok("raycast referent carries planId for routing", ctx.planId === "pl_banded_main");
  ok("the centre point IS inside the resolved box", boxContains(taskNode.box, boxCenter(taskNode.box)));
}

// MULTI-ORCHESTRATOR SWITCHING: centring over two plans owned by DIFFERENT orchestrators
// resolves to each plan in turn, so the composer target (plan.owner.pane_id) switches.
{
  const mainPlan = plans.find((p) => p.id === "pl_banded_main")!;
  const otherPlan = plans.find((p) => p.id === "pl_banded_other")!;
  ok("the two plans have different owners", mainPlan.owner.pane_id !== otherPlan.owner.pane_id);
  const otherFrame = layout.frames.find((f) => f.planId === "pl_banded_other")!;
  const otherTaskNode = otherFrame.nodes.find((n) => n.id === "t_other_a")!;
  const a = resolveCanvasContext({ overlay: null, plans, layout, center: boxCenter(taskNode.box) })!;
  const b = resolveCanvasContext({ overlay: null, plans, layout, center: boxCenter(otherTaskNode.box) })!;
  ok("centre over plan A → owner A", plans.find((p) => p.id === a.planId)!.owner.pane_id === "orchA");
  ok("centre over plan B → owner B (target switched)", plans.find((p) => p.id === b.planId)!.owner.pane_id === "orchB");
}

// centre far from everything + no overlay → still a valid referent or null, never a crash.
{
  const ctx = resolveCanvasContext({ overlay: null, plans, layout, center: farPoint });
  ok("far centre → valid context or null, never a crash", ctx === null || (!!ctx.planId && !!ctx.kind));
}

// ID PRESENT IN PAYLOAD: the wired marker carries plan + unit ids the orchestrator parses.
{
  const ctx = resolveCanvasContext({ overlay: { id: "t_parent", kind: "task", planId: "pl_banded_main" }, plans, layout, center: farPoint })!;
  const wire = buildCanvasMessage("please split this task", ctx);
  const marker = parseMarker(wire)!;
  ok("rides the pane marker envelope", !!marker && marker.kind === "pane");
  ok("marker routes by planId (pane attr)", marker.attrs.pane === "pl_banded_main");
  ok("payload carries the user text", marker.payload.includes("please split this task"));
  ok("payload carries the plan id", marker.payload.includes("plan pl_banded_main"));
  ok("payload carries the task id", marker.payload.includes("task t_parent"));
  ok("reference line is parseable", /\[Canvas context → plan pl_banded_main "[^"]*"; task t_parent "[^"]*"\]/.test(marker.payload));
}

// chip label names the referent + the real id; override drops to plan-level.
{
  const ctx = resolveCanvasContext({ overlay: { id: "t_parent", kind: "task", planId: "pl_banded_main" }, plans, layout, center: farPoint })!;
  ok("chip names unit + title + id", contextChipLabel(ctx) === "📍 Task · " + ctx.title + " · t_parent");
  const planLevel = toPlanContext(ctx, plans)!;
  ok("override → plan-level referent", planLevel.kind === "plan" && planLevel.id === "pl_banded_main");
  ok("plan reference has no unit clause", contextReference(planLevel) === '[Canvas context → plan pl_banded_main "' + planLevel.title + '"]');
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
