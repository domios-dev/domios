import type { OrchestratorRef, Plan } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { railAnchorOffsets } from "../settings";
import { flatSections } from "../sectionRows";
import type { Box, Point } from "./geometry";
import { unionBox } from "./geometry";
import { footprintFor } from "./nodeFootprints";
import { roundedRectHull, zoneColor } from "./sectionZones";
import { layoutZoneGrid } from "./zoneGrid";
import { UNSECTIONED } from "./taskCloud";
import type { IntraPlanLayout, LaidOutEdge, LaidOutNode, SectionZone } from "./types";

export function ownerKeyOf(owner: OrchestratorRef): string {
  return owner.session_id ?? owner.tab_id ?? owner.title ?? "__unowned__";
}

function sectionLabel(sectionId: string, titles: ReadonlyMap<string, string>): string | null {
  const t = titles.get(sectionId)?.trim();
  return t && t !== sectionId ? t : null;
}

function handoffSeq(content: unknown): number {
  const seq = (content as { seq?: unknown } | null)?.seq;
  return typeof seq === "number" ? seq : 0;
}

export function layoutPlanIntra(plan: Plan, s: CanvasLayoutSettings, titleOverrides?: ReadonlyMap<string, string>): IntraPlanLayout {
  const sectionTitles: ReadonlyMap<string, string> = titleOverrides ?? new Map(flatSections(plan).map((sec) => [sec.id, sec.title] as const));
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];
  const anchors = new Map<string, Point>();
  const extraPoints: Point[] = [];

  const flat = flatSections(plan);
  const hasSpec = !!plan.primary_spec_id;

  const planBox: Box = { x: 0, y: 0, width: s.planWidth, height: s.planHeight };
  const axisY = s.planHeight / 2;
  const specBox: Box = { x: -(s.columnGapX + s.specNodeW), y: axisY - s.specNodeH / 2, width: s.specNodeW, height: s.specNodeH };

  if (hasSpec) nodes.push({ id: `${plan.id}::spec`, kind: "spec", planId: plan.id, box: specBox, iteration: null, manual: false });
  nodes.push({ id: plan.id, kind: "plan", planId: plan.id, box: planBox, iteration: null, manual: false });

  const planOffsets = railAnchorOffsets(flat.length, s.planHeight, s);
  if (hasSpec) {
    edges.push({
      id: `${plan.id}~specplan`,
      kind: "spec-plan",
      fromPoint: { x: specBox.x + specBox.width, y: specBox.y + specBox.height / 2 },
      toPoint: { x: planBox.x, y: planBox.y + planBox.height / 2 },
    });
  }
  flat.forEach((section, i) => {
    anchors.set(section.id, { x: planBox.x + planBox.width, y: planBox.y + planOffsets[i] });
  });

  const grid = layoutZoneGrid(plan, s, anchors, sectionTitles);
  const fieldLeft = planBox.width + s.columnGapX;
  const fieldTop = axisY;
  for (const n of grid.nodes) {
    nodes.push({ ...n, box: { ...n.box, x: n.box.x + fieldLeft, y: n.box.y + fieldTop } });
  }

  const zones: SectionZone[] = [];
  for (const [sectionId, box] of grid.zoneBoxes) {
    const worldBox: Box = { x: box.x + fieldLeft, y: box.y + fieldTop, width: box.width, height: box.height };
    const labelAnchor = grid.groups.find((g) => g.sectionId === sectionId)?.labelAnchor;
    zones.push({
      sectionId,
      label: sectionId === UNSECTIONED ? null : sectionLabel(sectionId, sectionTitles),
      color: zoneColor(sectionId),
      hull: roundedRectHull(worldBox, s.zoneCornerRadius),
      labelAnchor: labelAnchor
        ? { x: labelAnchor.x + fieldLeft, y: labelAnchor.y + fieldTop }
        : { x: worldBox.x + worldBox.width / 2, y: worldBox.y + s.zonePad },
      box: worldBox,
    });
  }

  for (const e of grid.edges) edges.push(e);
  for (const pt of grid.extraPoints) extraPoints.push(pt);

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

  for (const el of plan.elements) {
    if (el.kind === "handoff") continue;
    if (el.canvas.x == null || el.canvas.y == null) continue;
    const fp = footprintFor(el.kind, el, s);
    const box: Box = { x: el.canvas.x, y: el.canvas.y, width: fp.width, height: fp.height };
    nodes.push({ id: el.id, kind: el.kind, planId: plan.id, box, iteration: null, manual: true });
  }

  const autoBoxes = nodes.map((n) => n.box);
  const zoneBoxes = zones.map((z) => z.box).filter(Boolean) as Box[];
  const content = unionBox([
    ...autoBoxes,
    ...zoneBoxes,
    ...[...anchors.values(), ...extraPoints].map((p) => ({ x: p.x, y: p.y, width: 0, height: 0 })),
  ]);
  const dx = s.framePadding;
  const dy = s.framePadding;
  const width = content.x + content.width + s.framePadding - Math.min(0, content.x + dx);
  const height = content.y + content.height + s.framePadding - Math.min(0, content.y + dy);
  for (let i = 0; i < nodes.length; i++) {
    nodes[i] = { ...nodes[i], box: { ...nodes[i].box, x: nodes[i].box.x + dx, y: nodes[i].box.y + dy } };
  }
  for (const [k, p] of anchors) anchors.set(k, { x: p.x + dx, y: p.y + dy });
  for (const e of edges) {
    if (e.fromPoint) e.fromPoint = { x: e.fromPoint.x + dx, y: e.fromPoint.y + dy };
    if (e.toPoint) e.toPoint = { x: e.toPoint.x + dx, y: e.toPoint.y + dy };
  }
  for (const z of zones) {
    z.hull = z.hull.map((p) => ({ x: p.x + dx, y: p.y + dy }));
    z.labelAnchor = { x: z.labelAnchor.x + dx, y: z.labelAnchor.y + dy };
    if (z.box) z.box = { ...z.box, x: z.box.x + dx, y: z.box.y + dy };
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
    zones,
  };
}
