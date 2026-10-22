import type { ViewportTransform } from "@codeterm/canvas-camera";
import type { Box } from "./geometry";
import { boxesOverlap } from "./geometry";
import type { WorkspaceLayout } from "./types";

export interface CanvasSize {
  width: number;
  height: number;
}

// World-space rect currently framed by the viewport, expanded by padding (screen px).
export function viewportWorldRect(t: ViewportTransform, canvas: CanvasSize, padding = 0): Box {
  return {
    x: (-t.x - padding) / t.scale,
    y: (-t.y - padding) / t.scale,
    width: (canvas.width + padding * 2) / t.scale,
    height: (canvas.height + padding * 2) / t.scale,
  };
}

// Virtualization (§6.1): only nodes intersecting the viewport (+ margin) are mounted.
export function visibleNodeIds(
  layout: WorkspaceLayout,
  t: ViewportTransform,
  canvas: CanvasSize,
  padding = 0,
): string[] {
  const rect = viewportWorldRect(t, canvas, padding);
  const ids: string[] = [];
  for (const f of layout.frames) {
    if (!boxesOverlap(f.box, rect)) continue;
    for (const n of f.nodes) if (boxesOverlap(n.box, rect)) ids.push(n.id);
  }
  return ids;
}

export function visibleFrameIds(
  layout: WorkspaceLayout,
  t: ViewportTransform,
  canvas: CanvasSize,
  padding = 0,
): string[] {
  const rect = viewportWorldRect(t, canvas, padding);
  return layout.frames.filter((f) => boxesOverlap(f.box, rect)).map((f) => f.planId);
}
