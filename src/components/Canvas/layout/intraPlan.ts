import type { OrchestratorRef, Plan } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { planModeHeight, railAnchorOffsets } from "../settings";
import { flatSections } from "../sectionRows";
import type { Box, Point } from "./geometry";
import { unionBox } from "./geometry";
import { layoutTaskBands } from "./taskBands";
import type { IntraPlanLayout, LaidOutEdge, LaidOutNode, NodeModeResolver } from "./types";

export function ownerKeyOf(owner: OrchestratorRef): string {
  return owner.session_id ?? owner.pane_id ?? owner.title ?? "__unowned__";
}

const DOCUMENT_MODE: NodeModeResolver = () => "document";

// ONE SectionedPlanNode (§22.2/§23.1) + banded task field. The plan node is a
// FIXED-size frame per manual mode (§22.1); each section emits a rail OUTPUT
// anchor on the node's right edge (stable Y), tasks band off those anchors.
export function layoutPlanIntra(
  plan: Plan,
  s: CanvasLayoutSettings,
  modeFor: NodeModeResolver = DOCUMENT_MODE,
): IntraPlanLayout {
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];
  const anchors = new Map<string, Point>();
  const extraPoints: Point[] = [];

  const flat = flatSections(plan);
  const planMode = modeFor(plan.id, "plan", plan.id);
  const planHeight = planModeHeight(planMode, s);

  const planBox: Box = { x: 0, y: 0, width: s.planWidth, height: planHeight };
  nodes.push({ id: plan.id, kind: "plan", planId: plan.id, box: planBox, iteration: null, manual: false });

  // Rail OUTPUT anchors (§23.1): one per section on the node's right edge, evenly
  // distributed so they always fit the fixed frame and stay stable per (i, mode) →
  // stable task edges. These replace the old plan-row + spec-row anchors.
  const offsets = railAnchorOffsets(flat.length, planHeight, s);
  flat.forEach((section, i) => {
    anchors.set(section.id, { x: planBox.x + planBox.width, y: planBox.y + offsets[i] });
  });

  const taskX = planBox.x + planBox.width + s.columnGapX;

  // Banded multi-column task sweep (§20.3) anchored to the rail outputs.
  const band = layoutTaskBands(plan, s, modeFor, anchors, taskX);
  nodes.push(...band.nodes);
  edges.push(...band.edges);
  extraPoints.push(...band.extraPoints);

  // Free-form elements (open registry kinds) render only when explicitly placed;
  // unpositioned auto-layout is deferred. Manual coords keep determinism.
  for (const el of plan.elements) {
    if (el.canvas.x == null || el.canvas.y == null) continue;
    const box: Box = { x: el.canvas.x, y: el.canvas.y, width: s.taskWidth, height: s.taskMinHeight };
    nodes.push({ id: el.id, kind: el.kind, planId: plan.id, box, iteration: null, manual: true });
  }

  const autoBoxes = nodes.map((n) => n.box);
  const content = unionBox([
    ...autoBoxes,
    ...[...anchors.values(), ...extraPoints].map((p) => ({ x: p.x, y: p.y, width: 0, height: 0 })),
  ]);
  const width = content.width + s.framePadding * 2;
  const height = content.height + s.framePadding * 2;

  // Shift everything by framePadding so nothing sits on the frame edge.
  const dx = s.framePadding - content.x;
  const dy = s.framePadding - content.y;
  for (let i = 0; i < nodes.length; i++) {
    nodes[i] = { ...nodes[i], box: { ...nodes[i].box, x: nodes[i].box.x + dx, y: nodes[i].box.y + dy } };
  }
  for (const [k, p] of anchors) anchors.set(k, { x: p.x + dx, y: p.y + dy });
  for (const e of edges) {
    if (e.fromPoint) e.fromPoint = { x: e.fromPoint.x + dx, y: e.fromPoint.y + dy };
    if (e.toPoint) e.toPoint = { x: e.toPoint.x + dx, y: e.toPoint.y + dy };
  }

  return {
    planId: plan.id,
    title: plan.title,
    owner: plan.owner,
    ownerKey: ownerKeyOf(plan.owner),
    updatedAt: plan.updated_at,
    width,
    height,
    nodes,
    edges,
    anchors,
  };
}
