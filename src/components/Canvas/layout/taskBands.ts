import type { Plan, Task, TaskStatus } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { taskFootprint } from "./taskFootprint";
import type { Box, Point } from "./geometry";
import type { LaidOutEdge, LaidOutNode } from "./types";

// §32.3/§32.10 spatial hierarchy: the cloud's vertical axis encodes status then
// priority — in_progress rises (top), pending/blocked sit mid, done/cancelled sink
// (bottom). Status is encoded by POSITION, never opacity.
const STATUS_BUCKET_MS = 60_000;
function shelfRank(status: TaskStatus): number {
  if (status === "in_progress") return 0;
  if (status === "done" || status === "cancelled") return 2;
  return 1;
}

function priorityRank(p: Task["priority"]): number {
  switch (p) {
    case "p0": return 0;
    case "p1": return 1;
    case "p3": return 3;
    default: return 2; // p2 / undefined
  }
}

// Cloud sort key (statusShelf, priority, status_changed_at bucket, createdAt, id):
// shelf+priority place the slot; a status change moves the task to its new vertical
// slot (~300ms animated) with no global reshuffle, no jitter while typing.
function cloudSort(a: Task, b: Task): number {
  const sr = shelfRank(a.status) - shelfRank(b.status);
  if (sr) return sr;
  const pr = priorityRank(a.priority) - priorityRank(b.priority);
  if (pr) return pr;
  const ab = Math.floor((a.status_changed_at ?? 0) / STATUS_BUCKET_MS);
  const bb = Math.floor((b.status_changed_at ?? 0) / STATUS_BUCKET_MS);
  if (ab !== bb) return bb - ab; // most-recently-active first within a shelf
  const ac = a.created_at ?? 0;
  const bc = b.created_at ?? 0;
  if (ac !== bc) return ac - bc;
  return a.id.localeCompare(b.id);
}

export interface TaskBandLayout {
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  extraPoints: Point[];
}

interface ChildItem {
  task: Task;
  w: number;
  h: number;
}

interface Cell {
  task: Task;
  order: number;
  bandRowY: number; // primary covered section's rail-output Y (or +Inf = standalone)
  w: number;
  h: number;
  childItems: ChildItem[];
  height: number; // full cell footprint (card + subtask stack)
}

// Banded multi-column task sweep (§20.3/§23.2): tasks band by primary covered
// section, pack into a bounded grid right of the plan node, top-down non-overlap.
// Each cell is a FIXED size per its manual mode (steps are INTERIOR to the card,
// not separate nodes); subtasks form a super-cell stacked under the parent.
export function layoutTaskBands(
  plan: Plan,
  s: CanvasLayoutSettings,
  anchors: Map<string, Point>,
  taskX: number,
): TaskBandLayout {
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

  const cells: Cell[] = topLevel.map((task, order) => {
    const children = childrenByParent.get(task.id) ?? [];
    const fp = taskFootprint(task, children, s);
    const w = fp.width;
    const h = fp.height;
    const childItems: ChildItem[] = children.map((c) => {
      const cs = taskFootprint(c, [], s);
      return { task: c, w: cs.width, h: cs.height };
    });
    let height = h;
    if (childItems.length) {
      height += s.subtaskHeaderGap + s.subtaskGroupPad * 2 - s.subtaskGapY;
      for (const ci of childItems) height += ci.h + s.subtaskGapY;
    }
    const cellW = Math.max(w, ...childItems.map((c) => c.w));
    const primarySid = task.section_ids.find((sid) => anchors.has(sid));
    return {
      task,
      order,
      bandRowY: primarySid != null ? anchors.get(primarySid)!.y : Number.POSITIVE_INFINITY,
      w: cellW,
      h,
      childItems,
      height,
    };
  });

  const colWidth = cells.length ? Math.max(...cells.map((c) => c.w)) : s.taskWidth;
  const colPitch = colWidth + s.taskColGap;

  // §32.10 task CLOUD: a 2D cluster right of the plan, sorted so the VERTICAL axis
  // encodes status/priority — active/high-priority at the top, done sinking to the
  // bottom. Column-major fill bounded by cloudMaxH: when a column would overflow
  // the bounded height, work continues in the next column rightward (never an
  // infinite vertical портянka). Deterministic — same data → same slots.
  const sorted = [...cells].sort((a, b) => cloudSort(a.task, b.task) || a.order - b.order);
  let col = 0;
  let y = 0;
  for (const cell of sorted) {
    if (y > 0 && y + cell.height > s.cloudMaxH) {
      col++;
      y = 0;
    }
    placeCell(cell, taskX + col * colPitch, y);
    y += cell.height + s.taskGapY;
  }

  return { nodes, edges, extraPoints };

  function placeCell(cell: Cell, x: number, y: number): void {
    const { task, w, h, childItems } = cell;
    nodes.push({ id: task.id, kind: "task", planId: plan.id, box: { x, y, width: w, height: h }, iteration: null, manual: false });

    for (const sid of task.section_ids) {
      const anchor = anchors.get(sid);
      if (anchor) edges.push({ id: `${sid}~${task.id}`, kind: "section-task", fromPoint: anchor, to: task.id });
    }

    if (!childItems.length) return;

    const groupTop = y + h + s.subtaskHeaderGap;
    let childY = groupTop + s.subtaskGroupPad;
    const placed: { item: ChildItem; box: Box }[] = [];
    for (const ci of childItems) {
      placed.push({ item: ci, box: { x: x + s.subtaskIndentX, y: childY, width: ci.w, height: ci.h } });
      childY += ci.h + s.subtaskGapY;
    }
    const groupBottom = childY - s.subtaskGapY + s.subtaskGroupPad;
    nodes.push({
      id: `${task.id}::subgroup`,
      kind: "subtask-group",
      planId: plan.id,
      box: { x: x + s.subtaskIndentX - s.subtaskGroupPad, y: groupTop, width: cell.w + s.subtaskGroupPad * 2, height: groupBottom - groupTop },
      iteration: null,
      manual: false,
    });
    for (const { item, box } of placed) {
      nodes.push({ id: item.task.id, kind: "task", planId: plan.id, box, iteration: null, manual: false });
      edges.push({ id: `${task.id}>${item.task.id}`, kind: "subtask", from: task.id, to: item.task.id });
    }
  }
}
