// Single source of every layout constant (§6.2). The layout engine reads ONLY
// from this object — no magic numbers inline. Strategy is swappable, not just numbers.

import type { TaskStatus } from "./types";

export type LayoutStrategy = "lanes" | "cluster" | "radial";

// Centralized per §6.2: task status palette lives here, not inline in TaskCard.
export const TASK_STATUS_COLORS: Record<TaskStatus, string> = {
  pending: "rgba(148, 163, 184, 0.85)",
  in_progress: "rgba(99, 102, 241, 0.9)",
  blocked: "rgba(248, 113, 113, 0.95)",
  done: "rgba(74, 222, 128, 0.9)",
  cancelled: "rgba(100, 116, 139, 0.75)",
};

export interface CanvasLayoutSettings {
  cardWidth: number;
  cardMinHeight: number;
  cardMaxHeight: number;

  sectionWidth: number;
  sectionIndentX: number;
  sectionGapY: number;
  taskGapY: number;

  laneWidth: number;
  laneGapX: number;
  bandGapX: number;

  framePadding: number;
  frameGap: number;
  frameMinWidth: number;
  rowMaxWidth: number;
  ownerClusterGapY: number;

  chainWrapWidth: number;
  clusterRadius: number;
  collisionPadding: number;

  milestoneNodeSize: number;
  milestoneGapX: number;
  milestoneGapY: number;

  zoomFarThreshold: number;
  zoomNearThreshold: number;
  edgeCurve: number;

  taskScale: number;
  milestoneNodeScale: number;
  milestoneGroupScale: number;

  focusZoneRadius: number;
  cullPadding: number;

  layoutStrategy: LayoutStrategy;
}

export const DEFAULT_CANVAS_SETTINGS: CanvasLayoutSettings = {
  cardWidth: 240,
  cardMinHeight: 64,
  cardMaxHeight: 320,

  sectionWidth: 220,
  sectionIndentX: 18,
  sectionGapY: 16,
  taskGapY: 28,

  laneWidth: 150,
  laneGapX: 24,
  bandGapX: 120,

  framePadding: 40,
  frameGap: 80,
  frameMinWidth: 360,
  rowMaxWidth: 6000,
  ownerClusterGapY: 200,

  chainWrapWidth: 640,
  clusterRadius: 90,
  collisionPadding: 24,

  milestoneNodeSize: 96,
  milestoneGapX: 24,
  milestoneGapY: 20,

  zoomFarThreshold: 0.45,
  zoomNearThreshold: 1.1,
  edgeCurve: 0.3,

  taskScale: 0.85,
  milestoneNodeScale: 1,
  milestoneGroupScale: 1,

  focusZoneRadius: 220,
  cullPadding: 300,

  layoutStrategy: "lanes",
};

export const CANVAS_LAYOUT_SETTING_KEYS = Object.keys(
  DEFAULT_CANVAS_SETTINGS,
) as (keyof CanvasLayoutSettings)[];
