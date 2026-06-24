import type { CSSProperties } from "react";

// Coarse camera-scale LOD, still used by MilestoneBubble. §26 worlds (W0–W3) drive
// the per-node content bands and supersede this for spec/plan/task cards.
export type CanvasLod = "far" | "mid" | "near";

export interface CanvasCardStyleProps {
  style: CSSProperties;
  lod?: CanvasLod;
  onMouseDown?: (e: React.MouseEvent) => void;
  dragging?: boolean;
}
