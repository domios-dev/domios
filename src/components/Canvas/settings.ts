// Single source of every layout constant. The layout engine and cards read ONLY
// from this object — no magic numbers inline. Plan-node row geometry is shared
// between the layout (anchor math) and PlanNode card (rendering) via these keys.

import type { TaskStatus } from "./types";

export type LayoutStrategy = "lanes" | "cluster" | "radial";

export const TASK_STATUS_COLORS: Record<TaskStatus, string> = {
  pending: "rgba(148, 163, 184, 0.85)",
  in_progress: "rgba(129, 140, 248, 0.92)",
  blocked: "rgba(239, 130, 130, 0.92)",
  done: "rgba(82, 180, 145, 0.9)",
  cancelled: "rgba(100, 116, 139, 0.7)",
};

// Milestone kind lanes: ONE coherent indigo→periwinkle family at a consistent
// value — distinguishable by hue step, never a rainbow of unrelated colors.
export const MILESTONE_LANE_COLORS = [
  "rgba(129, 140, 248, 0.92)",
  "rgba(120, 150, 240, 0.92)",
  "rgba(140, 138, 232, 0.92)",
  "rgba(112, 160, 224, 0.92)",
  "rgba(155, 142, 236, 0.92)",
  "rgba(126, 132, 226, 0.92)",
];

export interface CanvasLayoutSettings {
  // §24.3 un-merge: TWO connected sectioned nodes, SPEC | PLAN, plus the task
  // field. FIXED frames (§24.1) — zoom is camera-only; content adapts (content-LOD).
  // Spec + Plan share sectionNodeH so their per-section rows line up for 1:1 edges.
  specNodeW: number;
  planWidth: number;
  sectionNodeH: number;
  taskW: number;
  taskH: number;
  planHeaderHeight: number;
  planRowHeight: number;
  planFooterHeight: number;
  planRowIndent: number;
  railW: number;
  railRowMinH: number;
  planTileH: number;
  planCardH: number;
  planDocH: number;

  // Spec column: one expanded card per plan section.
  specWidth: number;
  specHeight: number;
  specHeaderHeight: number;
  specMinHeight: number;
  specMaxHeight: number;
  specSectionGapY: number;

  // Satellite task nodes.
  taskWidth: number;
  taskMinHeight: number;
  taskGapY: number;

  // Banded multi-column task sweep (§20.3): cap horizontal sprawl, bound the
  // vertical портянka. Tasks band by primary covered section, wrap across cols.
  maxTaskCols: number;
  taskColGap: number;
  bandPad: number;

  // FIXED task-cell size per MANUAL mode (§23.2) — content-independent (§22.1).
  // Tile = handle + status; Card = + digest + step thread; Document = full body +
  // interior step CARDS (the only +interior growth, bounded by stepsMaxVisible).
  taskTileW: number;
  taskTileH: number;
  taskCardW: number;
  taskCardH: number;
  taskDocW: number;
  taskDocH: number;

  // Thread & Coil step cards (§16.9) — INTERIOR to a Document task cell.
  stepCardH: number;
  stepGapY: number;
  stepsHeaderGap: number;
  stepsMaxVisible: number;

  // Subtask nesting (parent_task_id → children grouped under the parent).
  subtaskIndentX: number;
  subtaskGapY: number;
  subtaskHeaderGap: number;
  subtaskGroupPad: number;

  // Column gaps between spec | plan | tasks | clusters.
  columnGapX: number;
  clusterGapX: number;

  // Milestone bubble clusters.
  bubbleRadius: number;
  bubbleGapX: number;
  bubbleGapY: number;
  clusterLabelHeight: number;

  // Edge anchors.
  anchorInset: number;
  edgeCurve: number;

  // Orchestrator group wrapper.
  groupPadding: number;
  groupLabelHeight: number;
  groupGapX: number;
  groupGapY: number;

  // Frame packing.
  framePadding: number;
  frameGap: number;
  rowMaxWidth: number;

  // LOD + virtualization.
  zoomFarThreshold: number;
  zoomNearThreshold: number;
  focusZoneRadius: number;
  cullPadding: number;

  layoutStrategy: LayoutStrategy;
}

export const DEFAULT_CANVAS_SETTINGS: CanvasLayoutSettings = {
  specNodeW: 360,
  planWidth: 300,
  sectionNodeH: 460,
  taskW: 244,
  taskH: 132,
  planHeaderHeight: 80,
  planRowHeight: 34,
  planFooterHeight: 40,
  planRowIndent: 16,
  railW: 100,
  railRowMinH: 18,
  planTileH: 280,
  planCardH: 400,
  planDocH: 640,

  // Spec column: one expanded section card per plan section, left of the plan.
  // Wider per §16.8 for real markdown line length.
  specWidth: 440,
  specHeight: 348,
  specHeaderHeight: 30,
  specMinHeight: 60,
  specMaxHeight: 280,
  specSectionGapY: 16,

  taskWidth: 252,
  taskMinHeight: 58,
  taskGapY: 16,

  maxTaskCols: 3,
  taskColGap: 28,
  bandPad: 28,

  taskTileW: 200,
  taskTileH: 72,
  taskCardW: 260,
  taskCardH: 120,
  taskDocW: 260,
  taskDocH: 320,

  stepCardH: 58,
  stepGapY: 8,
  stepsHeaderGap: 8,
  stepsMaxVisible: 6,

  subtaskIndentX: 22,
  subtaskGapY: 12,
  subtaskHeaderGap: 14,
  subtaskGroupPad: 10,

  // Gutter rhythm: GUTTER intra-frame (spec→plan, plan→tasks), GUTTER*2 between
  // frames/owner groups, ~GUTTER for group padding. One base, no ad-hoc gaps.
  columnGapX: 32,
  clusterGapX: 16,

  bubbleRadius: 28,
  bubbleGapX: 12,
  bubbleGapY: 12,
  clusterLabelHeight: 17,

  anchorInset: 0,
  edgeCurve: 0.5,

  groupPadding: 40,
  groupLabelHeight: 34,
  groupGapX: 96,
  groupGapY: 120,

  framePadding: 24,
  frameGap: 96,
  rowMaxWidth: 9000,

  zoomFarThreshold: 0.45,
  zoomNearThreshold: 1.1,
  focusZoneRadius: 260,
  cullPadding: 300,

  layoutStrategy: "cluster",
};

export const CANVAS_LAYOUT_SETTING_KEYS = Object.keys(
  DEFAULT_CANVAS_SETTINGS,
) as (keyof CanvasLayoutSettings)[];

// Plan-node total height for a given visible section-row count. Shared so the
// layout's anchor math and the PlanNode card agree on row positions.
export function planNodeHeight(rowCount: number, s: CanvasLayoutSettings): number {
  return s.planHeaderHeight + rowCount * s.planRowHeight + s.planFooterHeight;
}

// §24.1: ONE fixed frame — spec and plan nodes share this height so their
// per-section rows align for the 1:1 edges. Content adapts to zoom, not the size.
export function sectionNodeHeight(s: CanvasLayoutSettings): number {
  return s.sectionNodeH;
}

// Interior height of a Document task cell's step CARDS (§16.9) — bounded: past
// stepsMaxVisible the step area scrolls inside the cell, so height never runs away.
export function stepsInteriorHeight(count: number, s: CanvasLayoutSettings): number {
  if (count <= 0) return 0;
  const visible = Math.min(count, s.stepsMaxVisible);
  return s.stepsHeaderGap + visible * s.stepCardH + (visible - 1) * s.stepGapY;
}

// §24.1: ONE fixed task-cell size — content (body, steps) adapts to zoom and
// scrolls inside this frame; the footprint never changes.
export function taskCellSize(s: CanvasLayoutSettings): { w: number; h: number } {
  return { w: s.taskW, h: s.taskH };
}

// Stable rail-row output-anchor Y offsets (local, from node top) for N sections.
// Evenly distributed in the rail body area so they ALWAYS fit the fixed frame
// (no overflow, §22.1) and are stable per (index, mode) → stable task edges.
export function railAnchorOffsets(count: number, nodeHeight: number, s: CanvasLayoutSettings): number[] {
  if (count <= 0) return [];
  const area = Math.max(0, nodeHeight - s.planHeaderHeight);
  const span = area / count;
  return Array.from({ length: count }, (_, i) => s.planHeaderHeight + (i + 0.5) * span);
}
