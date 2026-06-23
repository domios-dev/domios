import type { OrchestratorRef, Plan, Section, Task } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import type { Box } from "./geometry";
import { unionBox } from "./geometry";
import type { IntraPlanLayout, LaidOutEdge, LaidOutNode } from "./types";

export function ownerKeyOf(owner: OrchestratorRef): string {
  return owner.session_id ?? owner.pane_id ?? owner.title ?? "__unowned__";
}

interface FlatSection {
  section: Section;
  depth: number;
}

function flattenSections(sections: Section[], depth: number, out: FlatSection[]): void {
  for (const s of sections) {
    out.push({ section: s, depth });
    if (s.subsections.length) flattenSections(s.subsections, depth + 1, out);
  }
}

function manualBox(canvas: { x: number | null; y: number | null }, auto: Box): { box: Box; manual: boolean } {
  if (canvas.x != null && canvas.y != null) {
    return { box: { x: canvas.x, y: canvas.y, width: auto.width, height: auto.height }, manual: true };
  }
  return { box: auto, manual: false };
}

function clampManualPoint(
  x: number,
  y: number,
  size: { width: number; height: number },
  frameW: number,
  frameH: number,
  s: CanvasLayoutSettings,
): { x: number; y: number } {
  const minX = s.framePadding;
  const minY = s.framePadding;
  const maxX = Math.max(minX, frameW - s.framePadding - size.width);
  const maxY = Math.max(minY, frameH - s.framePadding - size.height);
  return {
    x: Math.max(minX, Math.min(maxX, x)),
    y: Math.max(minY, Math.min(maxY, y)),
  };
}

interface ChainResult {
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  width: number;
  height: number;
}

// Width-bounded milestone chain: declared kinds become lane columns, iterations
// stack as rows; repeated kinds fold downward so the chain spends vertical space.
function layoutChain(
  task: Task,
  planId: string,
  laneIndex: (kind: string) => number,
  laneCount: number,
  s: CanvasLayoutSettings,
): ChainResult {
  const size = s.milestoneNodeSize * s.milestoneNodeScale;
  const cellW = size + s.milestoneGapX;
  const cellH = size + s.milestoneGapY;
  const maxCols = Math.max(1, Math.floor(s.chainWrapWidth / cellW));
  const cols = Math.min(Math.max(laneCount, 1), maxCols);

  const filled = new Set<string>();
  let currentRow = 0;
  let maxRow = 0;
  const nodes: LaidOutNode[] = [];

  const ordered = [...task.milestones];
  for (const m of ordered) {
    const rawCol = laneIndex(m.kind);
    const col = rawCol % cols;
    const rowFold = Math.floor(rawCol / cols);
    let row = (m.iteration ?? currentRow) + rowFold;
    while (filled.has(`${row}:${col}`)) row++;
    filled.add(`${row}:${col}`);
    if (m.iteration == null) currentRow = row - rowFold;
    if (row > maxRow) maxRow = row;

    const auto: Box = { x: col * cellW, y: row * cellH, width: size, height: size };
    const { box, manual } = manualBox(m.canvas, auto);
    nodes.push({ id: m.id, kind: "milestone", planId, box, iteration: m.iteration, manual });
  }

  const edges: LaidOutEdge[] = [];
  const present = new Set(ordered.map((m) => m.id));
  for (const m of ordered) {
    for (const target of new Set(m.links)) {
      if (present.has(target)) edges.push({ id: `${m.id}->${target}`, from: m.id, to: target, kind: "milestone-chain" });
    }
  }

  const width = nodes.length ? cols * cellW - s.milestoneGapX : 0;
  const height = nodes.length ? (maxRow + 1) * cellH - s.milestoneGapY : 0;
  return { nodes, edges, width, height };
}

function buildLaneIndex(plan: Plan): { laneIndex: (kind: string) => number; laneCount: () => number } {
  const index = new Map<string, number>();
  plan.milestone_kinds.forEach((k, i) => index.set(k.key, i));
  let next = index.size;
  const laneIndex = (kind: string): number => {
    let i = index.get(kind);
    if (i == null) {
      i = next++;
      index.set(kind, i);
    }
    return i;
  };
  return { laneIndex, laneCount: () => Math.max(next, 1) };
}

// Intra-plan cascade (§6.1): sections left (portyanka), tasks middle, milestone
// chains right. Local coords with frame origin at (0,0); region packing offsets later.
export function layoutPlanIntra(plan: Plan, s: CanvasLayoutSettings): IntraPlanLayout {
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];

  const flat: FlatSection[] = [];
  flattenSections(plan.sections, 0, flat);
  const maxDepth = flat.reduce((m, f) => Math.max(m, f.depth), 0);

  const specBandX = s.framePadding;
  const specBandWidth = s.sectionWidth + maxDepth * s.sectionIndentX;
  const taskBandX = specBandX + specBandWidth + s.bandGapX;
  const taskWidth = s.cardWidth * s.taskScale;
  const chainBandX = taskBandX + taskWidth + s.bandGapX;

  let specY = s.framePadding;
  for (const { section, depth } of flat) {
    const auto: Box = {
      x: specBandX + depth * s.sectionIndentX,
      y: specY,
      width: s.sectionWidth,
      height: s.cardMinHeight,
    };
    const { box, manual } = manualBox(section.canvas, auto);
    nodes.push({ id: section.id, kind: "section", planId: plan.id, box, iteration: null, manual });
    specY += s.cardMinHeight + s.sectionGapY;
  }

  const { laneIndex, laneCount } = buildLaneIndex(plan);
  const sectionIds = new Set(flat.map((f) => f.section.id));

  let taskY = s.framePadding;
  for (const task of plan.tasks) {
    const chain = layoutChain(task, plan.id, laneIndex, laneCount(), s);
    const rowHeight = Math.max(s.cardMinHeight, chain.height);

    const auto: Box = { x: taskBandX, y: taskY, width: taskWidth, height: s.cardMinHeight };
    const { box, manual } = manualBox(task.canvas, auto);
    nodes.push({ id: task.id, kind: "task", planId: plan.id, box, iteration: null, manual });

    for (const sid of task.section_ids) {
      if (sectionIds.has(sid)) edges.push({ id: `${sid}~${task.id}`, from: sid, to: task.id, kind: "section-task" });
    }

    for (const cn of chain.nodes) {
      if (cn.manual) {
        nodes.push(cn);
      } else {
        nodes.push({ ...cn, box: { ...cn.box, x: cn.box.x + chainBandX, y: cn.box.y + taskY } });
      }
    }
    edges.push(...chain.edges);

    taskY += rowHeight + s.taskGapY;
  }

  // Free-form elements (§6.3): render only when explicitly positioned. Auto-layout
  // for unpositioned free nodes is deferred (Phase 3); manual coords keep determinism.
  for (const el of plan.elements) {
    if (el.canvas.x == null || el.canvas.y == null) continue;
    const box: Box = { x: el.canvas.x, y: el.canvas.y, width: s.cardWidth, height: s.cardMinHeight };
    nodes.push({ id: el.id, kind: el.kind, planId: plan.id, box, iteration: null, manual: true });
  }

  const autoBoxes = nodes.filter((n) => !n.manual).map((n) => n.box);
  const autoContent = unionBox(autoBoxes);
  const width = Math.max(s.frameMinWidth, autoContent.width + s.framePadding * 2);
  const height = autoContent.height + s.framePadding * 2;

  for (let i = 0; i < nodes.length; i++) {
    const n = nodes[i];
    if (!n.manual) continue;
    const c = clampManualPoint(n.box.x, n.box.y, n.box, width, height, s);
    nodes[i] = { ...n, box: { ...n.box, x: c.x, y: c.y } };
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
  };
}
