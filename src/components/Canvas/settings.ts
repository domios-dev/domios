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
  // Compact sectioned plan node.
  planWidth: number;
  planHeaderHeight: number;
  planRowHeight: number;
  planFooterHeight: number;
  planRowIndent: number;

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
  planWidth: 420,
  planHeaderHeight: 76,
  planRowHeight: 34,
  planFooterHeight: 40,
  planRowIndent: 16,

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
