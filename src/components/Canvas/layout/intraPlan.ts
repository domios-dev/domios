import type { OrchestratorRef, Plan } from "../types";
import type { CanvasViewMode } from "../cards/types";
import type { CanvasLayoutSettings } from "../settings";
import { planNodeHeight } from "../settings";
import { flatSections } from "../sectionRows";
import type { Box, Point } from "./geometry";
import { unionBox } from "./geometry";
import { estimateSpecHeight } from "./specSize";
import { layoutTaskBands } from "./taskBands";
import type { IntraPlanLayout, LaidOutEdge, LaidOutNode, NodeModeResolver } from "./types";

export function ownerKeyOf(owner: OrchestratorRef): string {
  return owner.session_id ?? owner.pane_id ?? owner.title ?? "__unowned__";
}

const DOCUMENT_MODE: NodeModeResolver = () => "document";

// Fixed designed frame height per non-document mode; document falls back to the
// content-fit estimate so each mode reserves exactly its own bounded space (§18).
function modeHeight(mode: CanvasViewMode, documentH: number, s: CanvasLayoutSettings, card: number): number {
  if (mode === "tile") return s.tileHeight;
  if (mode === "card") return card;
  return documentH;
}

function hasSpec(plan: Plan): boolean {
  const flat = flatSections(plan);
  return flat.length > 0;
}

// Compact-plan + spec + satellite-tasks + bubble-clusters (§2). Local coords with
// frame origin at (0,0); region packing offsets later. Section rows are anchors,
// not nodes — tasks connect by curved edges to the exact row.
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
  const specPresent = hasSpec(plan);

  const specX = 0;
  const planX = specPresent ? s.specWidth + s.columnGapX : 0;
  const planMode = modeFor(plan.id, "plan", plan.id);
  const planHeight = modeHeight(planMode, planNodeHeight(flat.length, s), s, s.planCardHeight);

  const planBox: Box = { x: planX, y: 0, width: s.planWidth, height: planHeight };
  nodes.push({ id: plan.id, kind: "plan", planId: plan.id, box: planBox, iteration: null, manual: false });

  // Per-section-row anchors: RIGHT edge → satellite tasks; LEFT edge → the
  // expanded spec-section card. Rows only exist in Document mode; in Card/Tile the
  // plan has no rows, so all section edges anchor to the node's mid-height edges.
  const leftAnchors = new Map<string, Point>();
  flat.forEach((section, i) => {
    const rowMidY =
      planMode === "document"
        ? planBox.y + s.planHeaderHeight + i * s.planRowHeight + s.planRowHeight / 2
        : planBox.y + planBox.height / 2;
    anchors.set(section.id, { x: planBox.x + planBox.width - s.anchorInset, y: rowMidY });
    leftAnchors.set(section.id, { x: planBox.x + s.anchorInset, y: rowMidY });
  });

  // Spec column: one expanded card per section, stacked left of the plan node,
  // each tied by a curved edge to its plan-node row.
  let specY = 0;
  for (const section of flat) {
    const specId = `${plan.id}::spec::${section.id}`;
    const specMode = modeFor(specId, "spec-section", plan.id);
    const h = modeHeight(specMode, estimateSpecHeight(section, s), s, s.specCardHeight);
    nodes.push({
      id: specId,
      kind: "spec-section",
      planId: plan.id,
      box: { x: specX, y: specY, width: s.specWidth, height: h },
      iteration: null,
      manual: false,
    });
    const left = leftAnchors.get(section.id)!;
    edges.push({ id: `${specId}~row`, kind: "spec-row", from: specId, toPoint: left });
    specY += h + s.specSectionGapY;
  }

  const taskX = planX + s.planWidth + s.columnGapX;

  // Banded multi-column task sweep (§20.3): bands by primary covered section,
  // bounded column grid, top-down non-overlap. Subtasks + step clusters are
  // interior cell height. Kills the single tall task портянka.
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

  // Vertically center the two columns (spec-section column vs plan+tasks block)
  // against each other so the content band is balanced, not top-weighted.
  const isBlock = (k: string) => k === "plan" || k === "task" || k === "milestone" || k === "cluster" || k === "subtask-group";
  const specBoxes = nodes.filter((n) => n.kind === "spec-section").map((n) => n.box);
  const blockBoxes = nodes.filter((n) => isBlock(n.kind)).map((n) => n.box);
  if (specBoxes.length && blockBoxes.length) {
    const specTop = Math.min(...specBoxes.map((b) => b.y));
    const specH = Math.max(...specBoxes.map((b) => b.y + b.height)) - specTop;
    const blockTop = Math.min(...blockBoxes.map((b) => b.y));
    const blockH = Math.max(...blockBoxes.map((b) => b.y + b.height)) - blockTop;
    const contentH = Math.max(specH, blockH);
    const specOff = (contentH - specH) / 2 - specTop;
    const blockOff = (contentH - blockH) / 2 - blockTop;
    for (let i = 0; i < nodes.length; i++) {
      const off = nodes[i].kind === "spec-section" ? specOff : isBlock(nodes[i].kind) ? blockOff : 0;
      if (off) nodes[i] = { ...nodes[i], box: { ...nodes[i].box, y: nodes[i].box.y + off } };
    }
    for (const [k, p] of anchors) anchors.set(k, { x: p.x, y: p.y + blockOff });
    // Every edge POINT is a plan-row/cluster anchor (block-side); the spec end of
    // a spec-row edge is a node and moves with specOff automatically.
    for (const e of edges) {
      if (e.fromPoint) e.fromPoint = { x: e.fromPoint.x, y: e.fromPoint.y + blockOff };
      if (e.toPoint) e.toPoint = { x: e.toPoint.x, y: e.toPoint.y + blockOff };
    }
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
