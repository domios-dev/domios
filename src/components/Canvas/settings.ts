// Single source of every layout constant. The layout engine and cards read ONLY
// from this object — no magic numbers inline. Plan-node row geometry is shared
// between the layout (anchor math) and PlanNode card (rendering) via these keys.

import type { TaskStatus } from "./types";

export type LayoutStrategy = "lanes" | "cluster" | "radial";

export const TASK_STATUS_COLORS: Record<TaskStatus, string> = {
  pending: "rgba(148, 163, 184, 0.85)",
  in_progress: "rgba(99, 102, 241, 0.9)",
  blocked: "rgba(248, 113, 113, 0.95)",
  done: "rgba(74, 222, 128, 0.9)",
  cancelled: "rgba(100, 116, 139, 0.75)",
};

// Milestone kind lanes: a tight, theme-leaning blue→violet→teal ramp at a
// consistent (muted) value — distinguishable without a circus rainbow.
export const MILESTONE_LANE_COLORS = [
  "rgba(129, 140, 248, 0.92)",
  "rgba(96, 165, 210, 0.92)",
  "rgba(125, 196, 184, 0.92)",
  "rgba(159, 150, 214, 0.92)",
  "rgba(112, 158, 214, 0.92)",
  "rgba(180, 162, 200, 0.92)",
];

export interface CanvasLayoutSettings {
  // Compact sectioned plan node.
  planWidth: number;
  planHeaderHeight: number;
  planRowHeight: number;
  planFooterHeight: number;
  planRowIndent: number;

  // Spec document node.
  specWidth: number;
  specHeight: number;

  // Satellite task nodes.
  taskWidth: number;
  taskMinHeight: number;
  taskGapY: number;

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
  planWidth: 308,
  planHeaderHeight: 76,
  planRowHeight: 34,
  planFooterHeight: 40,
  planRowIndent: 16,

  // Spec is a reference doc, NOT the hero: narrower + shorter than the plan mass.
  specWidth: 270,
  specHeight: 348,

  taskWidth: 252,
  taskMinHeight: 58,
  taskGapY: 16,

  // Gutter rhythm: GUTTER intra-frame (spec→plan, plan→tasks), GUTTER*2 between
  // frames/owner groups, ~GUTTER for group padding. One base, no ad-hoc gaps.
  columnGapX: 48,
  clusterGapX: 24,

  bubbleRadius: 30,
  bubbleGapX: 14,
  bubbleGapY: 26,
  clusterLabelHeight: 22,

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
