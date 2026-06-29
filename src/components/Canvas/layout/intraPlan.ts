import type { OrchestratorRef, Plan } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { railAnchorOffsets } from "../settings";
import { flatSections } from "../sectionRows";
import type { Box, Point } from "./geometry";
import { unionBox } from "./geometry";
import { layoutTaskCloud, UNSECTIONED } from "./taskCloud";
import { footprintFor } from "./nodeFootprints";
import { computeZonePolygon, zoneCentroid, zoneColor } from "./sectionZones";
import type { IntraPlanLayout, LaidOutEdge, LaidOutNode, SectionZone } from "./types";

export function ownerKeyOf(owner: OrchestratorRef): string {
  return owner.session_id ?? owner.pane_id ?? owner.title ?? "__unowned__";
}

// A readable zone name = the section's real TITLE, or null. Never a raw id: a task's
// main section_id can be an opaque hash that isn't a plan section (stale/mis-provided);
// showing that hex pill is the bug — omit the label instead.
function sectionLabel(sectionId: string, titles: ReadonlyMap<string, string>): string | null {
  const t = titles.get(sectionId)?.trim();
  return t && t !== sectionId ? t : null;
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
export function layoutPlanIntra(plan: Plan, s: CanvasLayoutSettings, titleOverrides?: ReadonlyMap<string, string>): IntraPlanLayout {
  // Section titles for label sizing + the name fallback: prefer the override (the
  // ORIGINAL plans' titles, since the layout plan's sections are spec-overlaid).
  const sectionTitles: ReadonlyMap<string, string> = titleOverrides ?? new Map(flatSections(plan).map((sec) => [sec.id, sec.title] as const));
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

  // Spec ⊥ Plan linkage is 1→N and lives in DATA (plan.spec_links); per-section
  // provenance renders INLINE as "From spec" chips. The canvas draws ONE calm
  // spec→plan connector (node-center to node-center), never a per-section wire fan.
  const planOffsets = railAnchorOffsets(flat.length, s.planHeight, s);
  if (hasSpec) {
    edges.push({
      id: `${plan.id}~specplan`,
      kind: "spec-plan",
      fromPoint: { x: specBox.x + specBox.width, y: specBox.y + specBox.height / 2 },
      toPoint: { x: planBox.x, y: planBox.y + planBox.height / 2 },
    });
  }
  // Per-section plan-side anchors (right edge) still feed the task cloud edges.
  flat.forEach((section, i) => {
    anchors.set(section.id, { x: planBox.x + planBox.width, y: planBox.y + planOffsets[i] });
  });

  // §2 task cloud to the RIGHT of the plan, centered on the plan mid-line. The
  // cloud is right-opening with a flat left edge; flush it to the plan by the
  // leftmost tile extent (not the full radius, which left a radius-wide void).
  const cloud = layoutTaskCloud(plan, s, anchors, sectionTitles);
  const leftExtent = cloud.nodes.reduce((m, n) => Math.min(m, n.box.x), 0);
  const diskCenter: Point = { x: planBox.width + s.columnGapX - leftExtent, y: axisY };
  for (const n of cloud.nodes) {
    nodes.push({ ...n, box: { ...n.box, x: n.box.x + diskCenter.x, y: n.box.y + diskCenter.y } });
  }

  // Section territories: each section's tasks (offset into frame coords) get a tight
  // rounded hull CLIPPED to its Voronoi cell (bisectors vs every other section's
  // centroid) — so the clusters tessellate like a PUZZLE: one whole cloud divided into
  // sub-clouds that meet at shared borders and never overlap. One zone per non-empty
  // section; the UNSECTIONED group stays unlabeled.
  const withBoxes = cloud.groups
    .filter((g) => g.boxes.length)
    .map((g) => ({
      sectionId: g.sectionId,
      boxes: g.boxes.map((b) => ({ ...b, x: b.x + diskCenter.x, y: b.y + diskCenter.y })),
      labelAnchor: g.labelAnchor ? { x: g.labelAnchor.x + diskCenter.x, y: g.labelAnchor.y + diskCenter.y } : undefined,
    }));
  const sites = withBoxes.map((g) => zoneCentroid(g.boxes));
  const zones: SectionZone[] = withBoxes.map((g, i) => {
    const { hull } = computeZonePolygon(g.boxes, sites[i], sites.filter((_, j) => j !== i));
    // The name sits at the RESERVED label-tile centre (cards packed around it); fall
    // back to the cluster centroid for the unsectioned group.
    return {
      sectionId: g.sectionId,
      label: g.sectionId === UNSECTIONED ? null : sectionLabel(g.sectionId, sectionTitles),
      color: zoneColor(g.sectionId),
      hull,
      labelAnchor: g.labelAnchor ?? sites[i],
    };
  });

  // Loop, not push(...spread): a spread of 100k+ edge args overflows the call stack.
  for (const e of cloud.edges) edges.push(e);
  for (const pt of cloud.extraPoints) extraPoints.push(pt);

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
    const fp = footprintFor(el.kind, el, s); // plugin-declared size, else the default box
    const box: Box = { x: el.canvas.x, y: el.canvas.y, width: fp.width, height: fp.height };
    nodes.push({ id: el.id, kind: el.kind, planId: plan.id, box, iteration: null, manual: true });
  }

  const autoBoxes = nodes.map((n) => n.box);
  const content = unionBox([
    ...autoBoxes,
    ...[...anchors.values(), ...extraPoints, ...zones.flatMap((z) => z.hull)].map((p) => ({ x: p.x, y: p.y, width: 0, height: 0 })),
  ]);
  // Pin the plan at (framePadding, framePadding) — never re-normalize to content min.
  // Re-centering on every bounds change shifted unrelated nodes when tasks grew.
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
