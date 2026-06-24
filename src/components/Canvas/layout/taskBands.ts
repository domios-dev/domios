import type { Plan, Task } from "../types";
import type { CanvasViewMode } from "../cards/types";
import type { CanvasLayoutSettings } from "../settings";
import type { Box, Point } from "./geometry";
import { buildLaneIndex, layoutCluster, type ClusterResult } from "./clusters";
import { estimateTaskHeight } from "./taskSize";
import type { LaidOutEdge, LaidOutNode, NodeModeResolver } from "./types";

export interface TaskBandLayout {
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  extraPoints: Point[];
}

interface ChildItem {
  task: Task;
  height: number;
  cluster: ClusterResult;
  selfH: number;
}

interface Cell {
  task: Task;
  order: number;
  band: string;
  bandRowY: number;
  parentHeight: number;
  parentCluster: ClusterResult | null;
  childItems: ChildItem[];
  height: number;
}

const STANDALONE_BAND = "__standalone__";

// Banded multi-column task sweep (§20.3): replace the single tall task column
// with a bounded grid anchored to section rows. Each task bands by its primary
// covered section; bands are swept top-down with non-overlap; subtasks + step
// clusters are INTERIOR cell height (no own grid cells) → portянka becomes a grid.
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

  const lanes = buildLaneIndex(plan);
  const taskHeight = (t: Task) => {
    const mode: CanvasViewMode = modeFor(t.id, "task", plan.id);
    if (mode === "tile") return s.tileHeight;
    if (mode === "card") return s.taskCardHeight;
    return estimateTaskHeight(t, s);
  };

  // Step cluster folds BELOW the card (interior height), so cells stay one column
  // wide (taskWidth) → a uniform grid. Label sits above the bubbles.
  const foldedClusterH = (c: ClusterResult) => (c.height ? s.clusterLabelHeight + c.height : 0);

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
    const parentHeight = taskHeight(task);
    const children = childrenByParent.get(task.id) ?? [];
    const parentCluster = children.length ? null : layoutCluster(task, plan.id, lanes, s);
    const childItems: ChildItem[] = children.map((c) => {
      const height = taskHeight(c);
      const cluster = layoutCluster(c, plan.id, lanes, s);
      const fold = foldedClusterH(cluster);
      return { task: c, height, cluster, selfH: height + (fold ? s.bubbleGapY + fold : 0) };
    });

    let cellH = parentHeight;
    if (parentCluster) {
      const fold = foldedClusterH(parentCluster);
      if (fold) cellH += s.bubbleGapY + fold;
    }
    if (childItems.length) {
      cellH += s.subtaskHeaderGap + s.subtaskGroupPad * 2 - s.subtaskGapY;
      for (const ci of childItems) cellH += ci.selfH + s.subtaskGapY;
    }

    const primarySid = task.section_ids.find((sid) => anchors.has(sid));
    return {
      task,
      order,
      band: primarySid ?? STANDALONE_BAND,
      bandRowY: primarySid != null ? anchors.get(primarySid)!.y : Number.POSITIVE_INFINITY,
      parentHeight,
      parentCluster,
      childItems,
      height: cellH,
    };
  });

  // Group into bands; stable intra-band order by creation index (§20.3.2 — not by
  // size, so a realtime insert appends without reshuffling the band).
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
  const colPitch = s.taskWidth + s.taskColGap;

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

  function pushClusterBelow(task: Task, x: number, top: number, cluster: ClusterResult): void {
    if (!cluster.nodes.length) return;
    const pad = 8;
    const labelH = s.clusterLabelHeight;
    const clusterX = x;
    const clusterY = top + labelH;
    nodes.push({
      id: `${task.id}::cluster`,
      kind: "cluster",
      planId: plan.id,
      box: { x: clusterX - pad, y: clusterY - pad - labelH, width: cluster.width + pad * 2, height: cluster.height + pad * 2 + labelH },
      iteration: null,
      manual: false,
    });
    for (const cn of cluster.nodes) {
      nodes.push({ ...cn, box: { ...cn.box, x: cn.box.x + clusterX, y: cn.box.y + clusterY } });
    }
    edges.push(...cluster.edges);
    const anchor: Point = { x: clusterX, y: clusterY };
    extraPoints.push(anchor, { x: clusterX + cluster.width, y: clusterY + cluster.height });
    edges.push({ id: `${task.id}~cluster`, kind: "task-cluster", from: task.id, toPoint: anchor });
  }

  function placeCell(cell: Cell, x: number, y: number): void {
    const { task, parentHeight, parentCluster, childItems } = cell;
    nodes.push({ id: task.id, kind: "task", planId: plan.id, box: { x, y, width: s.taskWidth, height: parentHeight }, iteration: null, manual: false });

    for (const sid of task.section_ids) {
      const anchor = anchors.get(sid);
      if (anchor) edges.push({ id: `${sid}~${task.id}`, kind: "section-task", fromPoint: anchor, to: task.id });
    }

    let interiorY = y + parentHeight;
    if (parentCluster && parentCluster.nodes.length) {
      interiorY += s.bubbleGapY;
      pushClusterBelow(task, x, interiorY, parentCluster);
      interiorY += foldedClusterH(parentCluster);
    }

    if (childItems.length) {
      const childX = x + s.subtaskIndentX;
      const childW = s.taskWidth - s.subtaskIndentX;
      const groupTop = interiorY + s.subtaskHeaderGap;
      let childY = groupTop + s.subtaskGroupPad;
      const placed: { item: ChildItem; box: Box }[] = [];
      for (const ci of childItems) {
        placed.push({ item: ci, box: { x: childX, y: childY, width: childW, height: ci.height } });
        childY += ci.selfH + s.subtaskGapY;
      }
      const groupBottom = childY - s.subtaskGapY + s.subtaskGroupPad;
      nodes.push({
        id: `${task.id}::subgroup`,
        kind: "subtask-group",
        planId: plan.id,
        box: { x: childX - s.subtaskGroupPad, y: groupTop, width: childW + s.subtaskGroupPad * 2, height: groupBottom - groupTop },
        iteration: null,
        manual: false,
      });
      for (const { item, box } of placed) {
        nodes.push({ id: item.task.id, kind: "task", planId: plan.id, box, iteration: null, manual: false });
        edges.push({ id: `${task.id}>${item.task.id}`, kind: "subtask", from: task.id, to: item.task.id });
        if (item.cluster.nodes.length) pushClusterBelow(item.task, box.x, box.y + item.height + s.bubbleGapY, item.cluster);
      }
    }
  }
}
