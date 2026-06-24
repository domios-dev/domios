import type { Plan, Task } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { taskFootprint } from "./taskFootprint";
import type { Box, Point } from "./geometry";
import type { LaidOutEdge, LaidOutNode } from "./types";

// worlds-v3 §2: golden-angle phyllotaxis pack. The irrational that gives maximal
// angular spacing (137.5°) so successive tiles never align into spokes.
export const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

// §2.2 inter-tile gap (g) folded into each tile's enclosing radius.
export const TILE_GAP = 10;

// §2.3 Alt C: innermost K slots reserved for in_progress tasks (≈1–2 rings). A
// fixed K is what makes the history-spiral index status-independent — going
// active/done ripples ≤K tiles, never the whole disk.
export const ACTIVE_CORE_K = 9;

function priorityRank(p: Task["priority"]): number {
  switch (p) {
    case "p0": return 0;
    case "p1": return 1;
    case "p3": return 3;
    default: return 2; // p2 / undefined
  }
}

// Stable creation rank over the given tasks by (created_at, id). A newly created
// task sorts last → outermost slot → existing tiles keep their indices (§31).
function creationRanks(tasks: Task[]): Map<string, number> {
  const sorted = [...tasks].sort(
    (a, b) => (a.created_at ?? 0) - (b.created_at ?? 0) || a.id.localeCompare(b.id),
  );
  const m = new Map<string, number>();
  sorted.forEach((t, i) => m.set(t.id, i));
  return m;
}

// Active-core rank among in_progress tasks (priority, then most-recently-active).
function activeRanks(tasks: Task[]): Map<string, number> {
  const active = tasks
    .filter((t) => t.status === "in_progress")
    .sort(
      (a, b) =>
        priorityRank(a.priority) - priorityRank(b.priority) ||
        (b.status_changed_at ?? 0) - (a.status_changed_at ?? 0) ||
        (a.created_at ?? 0) - (b.created_at ?? 0) ||
        a.id.localeCompare(b.id),
    );
  const m = new Map<string, number>();
  active.forEach((t, i) => m.set(t.id, i));
  return m;
}

// §2.3 two-zone sunflower index: active core (rank < K) at the eye, everything
// else on the history spiral at K + creationRank (independent of active count).
function slotIndex(task: Task, active: Map<string, number>, creation: Map<string, number>, k: number): number {
  const ar = active.get(task.id);
  if (ar != null && ar < k) return ar;
  return k + (creation.get(task.id) ?? 0);
}

export interface CloudItem {
  id: string;
  w: number;
  h: number;
  slot: number;
}

export interface PackedItem {
  id: string;
  cx: number;
  cy: number;
  w: number;
  h: number;
}

export interface CloudPack {
  items: PackedItem[];
  radius: number;
}

// §2.2 area-accumulated golden-angle pack with outward-only collision resolve.
// Tiles are placed in slot order; each only ever pushes ITSELF outward against
// already-placed tiles → deterministic, append-stable, non-overlapping. Returns
// centers relative to (0,0) and the disk radius R = max(r + ρ).
export function packCloud(items: CloudItem[], gap = TILE_GAP): CloudPack {
  const ordered = [...items].sort((a, b) => a.slot - b.slot || a.id.localeCompare(b.id));
  const placed: { cx: number; cy: number; rho: number }[] = [];
  const result: PackedItem[] = [];
  let cumArea = 0;
  let radius = 0;

  for (const it of ordered) {
    const rho = 0.5 * Math.hypot(it.w, it.h) + gap; // ρ = ½√(w²+h²) + g
    const area = Math.PI * rho * rho; // a = π·ρ²
    let r = Math.sqrt((cumArea + area / 2) / Math.PI); // areal-density radius
    const theta = it.slot * GOLDEN_ANGLE;
    let cx = r * Math.cos(theta);
    let cy = r * Math.sin(theta);

    // Outward-only: bump r by the deepest penetration until no circle overlaps.
    for (let guard = 0; guard < 4096; guard++) {
      let pen = 0;
      for (const p of placed) {
        const d = Math.hypot(cx - p.cx, cy - p.cy);
        const minD = rho + p.rho;
        if (d < minD - 1e-6) pen = Math.max(pen, minD - d);
      }
      if (pen <= 1e-6) break;
      r += pen;
      cx = r * Math.cos(theta);
      cy = r * Math.sin(theta);
    }

    placed.push({ cx, cy, rho });
    result.push({ id: it.id, cx, cy, w: it.w, h: it.h });
    cumArea += area;
    radius = Math.max(radius, r + rho);
  }

  return { items: result, radius };
}

export interface TaskCloudLayout {
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  extraPoints: Point[];
  radius: number;
}

interface ChildItem {
  task: Task;
  w: number;
  h: number;
}

interface Cell {
  task: Task;
  slot: number;
  cellW: number;
  cellH: number;
  cardW: number;
  cardH: number;
  childItems: ChildItem[];
}

// worlds-v3 §2: replaces the column-fill task field with a circle-packed disk.
// Top-level tasks are tiles; subtasks stack under their parent inside the cell
// footprint so the whole super-cell packs as one circle. Node positions are
// relative to the disk center (0,0); the caller offsets by diskCenter.
export function layoutTaskCloud(
  plan: Plan,
  s: CanvasLayoutSettings,
  anchors: Map<string, Point>,
): TaskCloudLayout {
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];
  const extraPoints: Point[] = [];

  const taskIds = new Set(plan.tasks.map((t) => t.id));
  const childrenByParent = new Map<string, Task[]>();
  for (const t of plan.tasks) {
    const pid = t.parent_task_id;
    if (pid && taskIds.has(pid)) {
      const arr = childrenByParent.get(pid);
      if (arr) arr.push(t);
      else childrenByParent.set(pid, [t]);
    }
  }
  const topLevel = plan.tasks.filter((t) => !t.parent_task_id || !taskIds.has(t.parent_task_id));

  const active = activeRanks(topLevel);
  const creation = creationRanks(topLevel);

  const cells: Cell[] = topLevel.map((task) => {
    const children = childrenByParent.get(task.id) ?? [];
    const fp = taskFootprint(task, children, s);
    const childItems: ChildItem[] = children.map((c) => {
      const cs = taskFootprint(c, [], s);
      return { task: c, w: cs.width, h: cs.height };
    });
    let cellH = fp.height;
    let childMaxW = 0;
    if (childItems.length) {
      cellH += s.subtaskHeaderGap + s.subtaskGroupPad * 2 - s.subtaskGapY;
      for (const ci of childItems) {
        cellH += ci.h + s.subtaskGapY;
        if (ci.w > childMaxW) childMaxW = ci.w;
      }
    }
    const cellW = Math.max(fp.width, childItems.length ? s.subtaskIndentX + childMaxW : 0);
    return { task, slot: slotIndex(task, active, creation, ACTIVE_CORE_K), cellW, cellH, cardW: fp.width, cardH: fp.height, childItems };
  });

  const pack = packCloud(
    cells.map((c) => ({ id: c.task.id, w: c.cellW, h: c.cellH, slot: c.slot })),
    TILE_GAP,
  );
  const byId = new Map(cells.map((c) => [c.task.id, c]));

  for (const p of pack.items) {
    const cell = byId.get(p.id)!;
    placeCell(cell, p.cx, p.cy);
  }

  return { nodes, edges, extraPoints, radius: pack.radius };

  function placeCell(cell: Cell, cx: number, cy: number): void {
    const { task, cardW, cardH, cellW, cellH, childItems } = cell;
    const left = cx - cellW / 2;
    const top = cy - cellH / 2;
    nodes.push({ id: task.id, kind: "task", planId: plan.id, box: { x: left, y: top, width: cardW, height: cardH }, iteration: null, manual: false });

    for (const sid of task.section_ids) {
      const anchor = anchors.get(sid);
      if (anchor) edges.push({ id: `${sid}~${task.id}`, kind: "section-task", fromPoint: anchor, to: task.id });
    }

    if (!childItems.length) return;

    const groupTop = top + cardH + s.subtaskHeaderGap;
    let childY = groupTop + s.subtaskGroupPad;
    const placed: { item: ChildItem; box: Box }[] = [];
    for (const ci of childItems) {
      placed.push({ item: ci, box: { x: left + s.subtaskIndentX, y: childY, width: ci.w, height: ci.h } });
      childY += ci.h + s.subtaskGapY;
    }
    const groupBottom = childY - s.subtaskGapY + s.subtaskGroupPad;
    nodes.push({
      id: `${task.id}::subgroup`,
      kind: "subtask-group",
      planId: plan.id,
      box: { x: left + s.subtaskIndentX - s.subtaskGroupPad, y: groupTop, width: cellW + s.subtaskGroupPad * 2, height: groupBottom - groupTop },
      iteration: null,
      manual: false,
    });
    for (const { item, box } of placed) {
      nodes.push({ id: item.task.id, kind: "task", planId: plan.id, box, iteration: null, manual: false });
      edges.push({ id: `${task.id}>${item.task.id}`, kind: "subtask", from: task.id, to: item.task.id });
    }
  }
}
