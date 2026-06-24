import type { Plan, Task } from "../types";
import type { CanvasViewMode } from "../cards/types";
import type { CanvasLayoutSettings } from "../settings";
import { taskCellSize } from "../settings";
import type { Box, Point } from "./geometry";
import type { LaidOutEdge, LaidOutNode, NodeModeResolver } from "./types";

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
  band: string;
  bandRowY: number;
  w: number;
  h: number;
  childItems: ChildItem[];
  height: number; // full cell footprint (card + subtask stack)
}

const STANDALONE_BAND = "__standalone__";

// Banded multi-column task sweep (§20.3/§23.2): tasks band by primary covered
// section, pack into a bounded grid right of the plan node, top-down non-overlap.
// Each cell is a FIXED size per its manual mode (steps are INTERIOR to the card,
// not separate nodes); subtasks form a super-cell stacked under the parent.
export function layoutTaskBands(
  plan: Plan,
  s: CanvasLayoutSettings,
  modeFor: NodeModeResolver,
  anchors: Map<string, Point>,
  taskX: number,
): TaskBandLayout {
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];
  const extraPoints: Point[] = [];

  const sizeOf = (t: Task) => {
    const mode: CanvasViewMode = modeFor(t.id, "task", plan.id);
    return taskCellSize(mode, t.milestones.length, s);
  };

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
    const { w, h } = sizeOf(task);
    const childItems: ChildItem[] = (childrenByParent.get(task.id) ?? []).map((c) => {
      const cs = sizeOf(c);
      return { task: c, w: cs.w, h: cs.h };
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
      band: primarySid ?? STANDALONE_BAND,
      bandRowY: primarySid != null ? anchors.get(primarySid)!.y : Number.POSITIVE_INFINITY,
      w: cellW,
      h,
      childItems,
      height,
    };
  });

  // Group into bands; stable intra-band order by creation index (§20.3.2).
  const bandMap = new Map<string, Cell[]>();
  for (const c of cells) {
    const arr = bandMap.get(c.band);
    if (arr) arr.push(c);
    else bandMap.set(c.band, [c]);
  }
  const bands = [...bandMap.values()];
  for (const arr of bands) arr.sort((a, b) => a.order - b.order);
  bands.sort((a, b) => a[0].bandRowY - b[0].bandRowY || a[0].order - b[0].order);

  const cols = Math.max(1, s.maxTaskCols);
  const colWidth = cells.length ? Math.max(...cells.map((c) => c.w)) : s.taskWidth;
  const colPitch = colWidth + s.taskColGap;

  // Top-down non-overlap sweep (§20.3.5): each band hugs its section-row Y, pushed
  // down only when the previous band already occupies that space.
  let cursorY = 0;
  for (const band of bands) {
    const subRows = Math.ceil(band.length / cols);
    const rowH: number[] = [];
    for (let r = 0; r < subRows; r++) {
      let h = 0;
      for (let c = 0; c < cols; c++) {
        const idx = r * cols + c;
        if (idx < band.length) h = Math.max(h, band[idx].height);
      }
      rowH.push(h);
    }
    const blockH = rowH.reduce((a, b) => a + b, 0) + Math.max(0, subRows - 1) * s.taskGapY;
    const anchored = Number.isFinite(band[0].bandRowY);
    const idealTop = (anchored ? band[0].bandRowY : cursorY) - blockH / 2;
    const top = Math.max(cursorY, idealTop);

    let rowTop = top;
    for (let r = 0; r < subRows; r++) {
      for (let c = 0; c < cols; c++) {
        const idx = r * cols + c;
        if (idx >= band.length) continue;
        placeCell(band[idx], taskX + c * colPitch, rowTop);
      }
      rowTop += rowH[r] + s.taskGapY;
    }
    cursorY = top + blockH + s.bandPad;
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
