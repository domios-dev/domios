
import type { TaskStatus } from "./types";

export type LayoutStrategy = "lanes" | "cluster" | "radial";

export const TASK_STATUS_COLORS: Record<TaskStatus, string> = {
  pending: "rgba(148, 163, 184, 0.85)",
  in_progress: "rgba(129, 140, 248, 0.92)",
  in_review: "rgba(245, 158, 11, 0.92)",
  blocked: "rgba(239, 130, 130, 0.92)",
  paused: "rgba(148, 163, 184, 0.65)",
  done: "rgba(82, 180, 145, 0.9)",
  cancelled: "rgba(100, 116, 139, 0.9)",
};

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
  specNodeW: number;
  specNodeH: number;
  planWidth: number;
  planHeight: number;
  planHeaderHeight: number;

  // physically wider so it crosses bands at wider appW. Hysteresis damps flicker.
  worldThresholds: [number, number, number];
  worldThresholdsSpec: [number, number, number];
  worldHysteresis: number;

  taskW: number; // base 1-col slot width (also the empty-board fallback)
  taskColStep: number; // px added per snapped column step
  taskMaxW: number;
  taskBaseH: number; // height intercept before volume terms
  taskMinH: number; // hard lower clamp
  taskMaxH: number;
  aspectFloorRatio: number;

  // Free-form element fallback box.
  taskWidth: number;
  taskMinHeight: number;

  // Subtask nesting (parent_task_id → children grouped under the parent).
  subtaskIndentX: number;
  subtaskGapY: number;
  subtaskHeaderGap: number;
  subtaskGroupPad: number;

  // §32.8 handoff timeline lane (chronological coins above the plan frame).
  handoffCoinW: number;
  handoffCoinH: number;
  handoffLaneGap: number;
  handoffLaneMargin: number;

  ghostPlanW: number;
  ghostPlanH: number;
  ghostPlanGap: number;
  ghostPlanStackGap: number;

  // Column gaps between spec | plan | tasks.
  columnGapX: number;

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

  // Canvas v3 zone-first task field.
  zoneTaskW: number;
  zoneTaskH: number;
  zoneTaskGap: number;
  zoneCols: number;
  zonePad: number;
  zoneGap: number;
  zoneLabelGap: number;
  zoneCornerRadius: number;

  // Camera zoom clamps (fit..400%).
  zoomMinScale: number;
  zoomMaxScale: number;
  zoomFitMargin: number;

  // LOD + virtualization.
  zoomFarThreshold: number;
  zoomNearThreshold: number;
  focusZoneRadius: number;
  cullPadding: number;

  layoutStrategy: LayoutStrategy;
}

export const DEFAULT_CANVAS_SETTINGS: CanvasLayoutSettings = {
  specNodeW: 440,
  specNodeH: 720,
  planWidth: 680,
  planHeight: 860,
  planHeaderHeight: 80,

  worldThresholds: [140, 300, 540],
  worldThresholdsSpec: [220, 440, 760],
  worldHysteresis: 0.1,

  // from zoom-drill (worlds), not a big default card. Bounded growth still applies.
  taskW: 176,
  taskColStep: 40,
  taskMaxW: 300,
  taskBaseH: 84,
  taskMinH: 104,
  taskMaxH: 300,
  aspectFloorRatio: 0.6,

  taskWidth: 280,
  taskMinHeight: 180,

  subtaskIndentX: 22,
  subtaskGapY: 12,
  subtaskHeaderGap: 14,
  subtaskGroupPad: 10,

  handoffCoinW: 210,
  handoffCoinH: 150,
  handoffLaneGap: 22,
  handoffLaneMargin: 44,

  ghostPlanW: 148,
  ghostPlanH: 48,
  ghostPlanGap: 20,
  ghostPlanStackGap: 10,

  // frames/owner groups, ~GUTTER for group padding. One base, no ad-hoc gaps.
  columnGapX: 36,

  anchorInset: 0,
  edgeCurve: 0.5,

  groupPadding: 40,
  groupLabelHeight: 34,
  groupGapX: 96,
  groupGapY: 120,

  framePadding: 28,
  frameGap: 72,
  rowMaxWidth: 9000,

  zoneTaskW: 176,
  zoneTaskH: 104,
  zoneTaskGap: 10,
  zoneCols: 4,
  zonePad: 14,
  zoneGap: 20,
  zoneLabelGap: 8,
  zoneCornerRadius: 16,

  zoomMinScale: 0.1,
  zoomMaxScale: 4,
  zoomFitMargin: 0.1,

  zoomFarThreshold: 0.32,
  zoomNearThreshold: 1.1,
  focusZoneRadius: 260,
  cullPadding: 300,

  layoutStrategy: "cluster",
};

export const CANVAS_LAYOUT_SETTING_KEYS = Object.keys(
  DEFAULT_CANVAS_SETTINGS,
) as (keyof CanvasLayoutSettings)[];

export function railAnchorOffsets(count: number, nodeHeight: number, s: CanvasLayoutSettings): number[] {
  if (count <= 0) return [];
  const area = Math.max(0, nodeHeight - s.planHeaderHeight);
  const span = area / count;
  return Array.from({ length: count }, (_, i) => s.planHeaderHeight + (i + 0.5) * span);
}
