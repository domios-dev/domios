import type { OrchestratorRef, Plan } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { railAnchorOffsets } from "../settings";
import { flatSections } from "../sectionRows";
import type { Box, Point } from "./geometry";
import { unionBox } from "./geometry";
import { layoutTaskCloud } from "./taskCloud";
import type { IntraPlanLayout, LaidOutEdge, LaidOutNode } from "./types";

export function ownerKeyOf(owner: OrchestratorRef): string {
  return owner.session_id ?? owner.pane_id ?? owner.title ?? "__unowned__";
}

function handoffSeq(content: unknown): number {
  const seq = (content as { seq?: unknown } | null)?.seq;
  return typeof seq === "number" ? seq : 0;
}

// worlds-v3 §1 plan-centric composition: PLAN is the focal mass at the fixed
// origin; SPEC sits to its LEFT (negative-x), the circle-packed task disk to its
// RIGHT — spec + disk vertically centered on the plan mid-line (a balanced
// triptych). Section rows distribute inside each node's OWN height, so the 1:1
// spec↔plan edges go diagonal across the differing Ys.
export function layoutPlanIntra(plan: Plan, s: CanvasLayoutSettings): IntraPlanLayout {
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];
  const anchors = new Map<string, Point>();
  const extraPoints: Point[] = [];

  const flat = flatSections(plan);
  // Spec ⊥ Plan: render a spec node ONLY when the plan is bound to a REAL Spec
  // entity. Without one, mirroring plan.sections into a "Specification" box is the
  // spec=plan conflation (visual duplication) — don't.
  const hasSpec = !!plan.primary_spec_id;

  const planBox: Box = { x: 0, y: 0, width: s.planWidth, height: s.planHeight };
  const axisY = s.planHeight / 2; // the composition's horizontal axis
  const specBox: Box = { x: -(s.columnGapX + s.specNodeW), y: axisY - s.specNodeH / 2, width: s.specNodeW, height: s.specNodeH };

  if (hasSpec) nodes.push({ id: `${plan.id}::spec`, kind: "spec", planId: plan.id, box: specBox, iteration: null, manual: false });
  nodes.push({ id: plan.id, kind: "plan", planId: plan.id, box: planBox, iteration: null, manual: false });

  // Per-section anchors at evenly-distributed offsets inside each node's own
  // height (§1: heights differ, so spec rows use spec offsets, plan rows use plan
  // offsets; the same-section edge is allowed to be diagonal).
  const specOffsets = railAnchorOffsets(flat.length, s.specNodeH, s);
  const planOffsets = railAnchorOffsets(flat.length, s.planHeight, s);
  flat.forEach((section, i) => {
    if (hasSpec) {
      edges.push({
        id: `${section.id}~specrow`,
        kind: "spec-row",
        fromPoint: { x: specBox.x + specBox.width, y: specBox.y + specOffsets[i] },
        toPoint: { x: planBox.x, y: planBox.y + planOffsets[i] },
      });
    }
    anchors.set(section.id, { x: planBox.x + planBox.width, y: planBox.y + planOffsets[i] });
  });

  // §2 circle-pack task disk to the RIGHT of the plan, centered on the plan
  // mid-line. The pack returns centers relative to (0,0) + the disk radius R;
  // diskCenter.x = planW + gap + R places the whole disk clear of the plan.
  const cloud = layoutTaskCloud(plan, s, anchors);
  const diskCenter: Point = { x: planBox.width + s.columnGapX + cloud.radius, y: axisY };
  for (const n of cloud.nodes) {
    nodes.push({ ...n, box: { ...n.box, x: n.box.x + diskCenter.x, y: n.box.y + diskCenter.y } });
  }
  edges.push(...cloud.edges);
  extraPoints.push(...cloud.extraPoints);

  // §32.8 handoff timeline lane: chronological reset coins ABOVE the plan frame
  // (never inside task clutter), linked [n-1]→[n] left→right by seq. Auto-placed
  // (handoffs carry no manual coords); deterministic by seq so the camera is stable.
  const handoffs = plan.elements
    .filter((el) => el.kind === "handoff")
    .map((el) => ({ el, seq: handoffSeq(el.content) }))
    .sort((a, b) => a.seq - b.seq || a.el.id.localeCompare(b.el.id));
  if (handoffs.length) {
    const laneY = -(s.handoffCoinH + s.handoffLaneMargin);
    const pitch = s.handoffCoinW + s.handoffLaneGap;
    handoffs.forEach(({ el }, i) => {
      const box: Box = { x: i * pitch, y: laneY, width: s.handoffCoinW, height: s.handoffCoinH };
      nodes.push({ id: el.id, kind: "handoff", planId: plan.id, box, iteration: null, manual: false });
      if (i > 0) edges.push({ id: `${handoffs[i - 1].el.id}>${el.id}`, kind: "handoff-chain", from: handoffs[i - 1].el.id, to: el.id });
    });
  }

  // Free-form elements (open registry kinds) render only when explicitly placed;
  // unpositioned auto-layout is deferred. Manual coords keep determinism.
  for (const el of plan.elements) {
    if (el.kind === "handoff") continue;
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
