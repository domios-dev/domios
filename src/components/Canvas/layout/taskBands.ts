import type { Plan, Task } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { taskFootprint } from "./taskFootprint";
import type { Box, Point } from "./geometry";
import type { LaidOutEdge, LaidOutNode } from "./types";

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

  const cols = Math.max(1, s.maxTaskCols);
  const colWidth = cells.length ? Math.max(...cells.map((c) => c.w)) : s.taskWidth;
  const colPitch = colWidth + s.taskColGap;

  // Anchor-hugging skyline column packing (§20.3 + height-bound fix): place each
  // cell at its section-row Y in whichever column keeps it HIGHEST (best hug),
  // pushing down only when every column is occupied there. Bounds the task-field
  // height to ~ΣcellH / cols instead of a single tall stack — no km-strip board.
  const sorted = [...cells].sort((a, b) => a.bandRowY - b.bandRowY || a.order - b.order);
  const colBottom = new Array(cols).fill(0);
  for (const cell of sorted) {
    const idealTop = Number.isFinite(cell.bandRowY) ? cell.bandRowY - cell.height / 2 : Math.min(...colBottom);
    let bestC = 0;
    let bestTop = Number.POSITIVE_INFINITY;
    for (let c = 0; c < cols; c++) {
      const top = Math.max(colBottom[c], idealTop);
      if (top < bestTop) {
        bestTop = top;
        bestC = c;
      }
    }
    placeCell(cell, taskX + bestC * colPitch, bestTop);
    colBottom[bestC] = bestTop + cell.height + s.taskGapY;
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
