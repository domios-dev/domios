import type { OrchestratorRef, Plan } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { sectionNodeHeight, railAnchorOffsets } from "../settings";
import { flatSections } from "../sectionRows";
import type { Box, Point } from "./geometry";
import { unionBox } from "./geometry";
import { layoutTaskBands } from "./taskBands";
import type { IntraPlanLayout, LaidOutEdge, LaidOutNode } from "./types";

export function ownerKeyOf(owner: OrchestratorRef): string {
  return owner.session_id ?? owner.pane_id ?? owner.title ?? "__unowned__";
}

// §24.3 three connected columns: SPEC | PLAN | tasks. Spec + plan are two fixed
// sectioned nodes sharing one height so their per-section rows align for the 1:1
// spec↔plan edges; plan's right-edge OUTPUT anchors feed the banded task field.
export function layoutPlanIntra(plan: Plan, s: CanvasLayoutSettings): IntraPlanLayout {
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];
  const anchors = new Map<string, Point>();
  const extraPoints: Point[] = [];

  const flat = flatSections(plan);
  const H = sectionNodeHeight(s);
  const hasSpec = flat.length > 0;

  const specBox: Box = { x: 0, y: 0, width: s.specNodeW, height: H };
  const planX = hasSpec ? s.specNodeW + s.columnGapX : 0;
  const planBox: Box = { x: planX, y: 0, width: s.planWidth, height: H };

  if (hasSpec) nodes.push({ id: `${plan.id}::spec`, kind: "spec", planId: plan.id, box: specBox, iteration: null, manual: false });
  nodes.push({ id: plan.id, kind: "plan", planId: plan.id, box: planBox, iteration: null, manual: false });

  // Per-section anchors at evenly-distributed offsets (stable, fit the fixed frame).
  // Spec right-edge dot → plan-row left edge (1:1, same section). Plan right-edge
  // OUTPUT → tasks. Spec + plan share H so these line up row-for-row.
  const offsets = railAnchorOffsets(flat.length, H, s);
  flat.forEach((section, i) => {
    const y = offsets[i];
    if (hasSpec) {
      edges.push({
        id: `${section.id}~specrow`,
        kind: "spec-row",
        fromPoint: { x: specBox.x + specBox.width, y: specBox.y + y },
        toPoint: { x: planBox.x, y: planBox.y + y },
      });
    }
    anchors.set(section.id, { x: planBox.x + planBox.width, y: planBox.y + y });
  });

  const taskX = planBox.x + planBox.width + s.columnGapX;

  // Banded multi-column task sweep (§20.3) anchored to the plan rail outputs.
  const band = layoutTaskBands(plan, s, anchors, taskX);
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
