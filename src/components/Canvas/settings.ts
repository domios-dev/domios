// Single source of every layout constant. The layout engine and cards read ONLY
// from this object — no magic numbers inline. Plan-node row geometry is shared
// between the layout (anchor math) and SectionNode card (rendering) via these keys.

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
  // worlds-v3 §1 plan-centric composition: PLAN is the focal mass (biggest in BOTH
  // axes), SPEC a smaller doc node to its LEFT, task disk to its RIGHT. Heights now
  // differ per node — railAnchorOffsets distributes rows inside each node's own
  // height, so 1:1 spec↔plan edges go diagonal across the differing Ys.
  specNodeW: number;
  specNodeH: number;
  planWidth: number;
  planHeight: number;
  planHeaderHeight: number;

  // §26 WORLDS appW thresholds (W0/W1/W2/W3 boundaries, in screen px). SPEC is
  // physically wider so it crosses bands at wider appW. Hysteresis damps flicker.
  worldThresholds: [number, number, number];
  worldThresholdsSpec: [number, number, number];
  worldHysteresis: number;

  // §32.4 data-driven task footprint: size ∝ work volume, fixed across zoom, grows
  // in BOTH axes (bounded). Width snaps to column slots; height carries magnitude
  // and never falls below aspectFloorRatio·width (no hollow letterboxes).
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

  // LOD + virtualization.
  zoomFarThreshold: number;
  zoomNearThreshold: number;
  focusZoneRadius: number;
  cullPadding: number;

  layoutStrategy: LayoutStrategy;
}

export const DEFAULT_CANVAS_SETTINGS: CanvasLayoutSettings = {
  // worlds-v3 §1 size hierarchy (hard invariant): plan > spec ≈ tasks, biggest in
  // both axes. A task's enclosing circle can never reach the smallest big node —
  // asserted by the unit test in settings.test.ts.
  specNodeW: 440,
  specNodeH: 720,
  planWidth: 680,
  planHeight: 860,
  planHeaderHeight: 80,

  // appW px boundaries (normal node / SPEC node). W2 = body-visible band (§26.2).
  // Tasks are small (§32.10) so thresholds sit lower; deep zoom (maxScale 4)
  // reaches W3 on a small tile (the zoom-drill that replaces big default cards).
  worldThresholds: [140, 300, 540],
  worldThresholdsSpec: [220, 440, 760],
  worldHysteresis: 0.1,

  // §32.10 tasks are SMALL by default (≈½ the section-node size) — detail comes
  // from zoom-drill (worlds), not a big default card. Bounded growth still applies.
  taskW: 156,
  taskColStep: 40,
  taskMaxW: 240,
  taskBaseH: 84,
  taskMinH: 88,
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

  // Gutter rhythm: GUTTER intra-frame (spec→plan, plan→tasks), GUTTER*2 between
  // frames/owner groups, ~GUTTER for group padding. One base, no ad-hoc gaps.
  columnGapX: 36,

  anchorInset: 0,
  edgeCurve: 0.5,

  groupPadding: 40,
  groupLabelHeight: 34,
  groupGapX: 96,
  groupGapY: 120,

  framePadding: 28,
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

// Stable rail-row output-anchor Y offsets (local, from node top) for N sections.
// Evenly distributed in the rail body area so they ALWAYS fit the fixed frame
// (no overflow, §22.1) and are stable per index → stable task edges.
export function railAnchorOffsets(count: number, nodeHeight: number, s: CanvasLayoutSettings): number[] {
  if (count <= 0) return [];
  const area = Math.max(0, nodeHeight - s.planHeaderHeight);
  const span = area / count;
  return Array.from({ length: count }, (_, i) => s.planHeaderHeight + (i + 0.5) * span);
}
