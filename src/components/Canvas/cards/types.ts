import type { CSSProperties } from "react";

export type CanvasLod = "far" | "mid" | "near";

// Discrete, hand-designed node representations (§18). NOT continuous text scaling:
// switching mode swaps the whole layout, text stays at a fixed readable size.
//   document — full markdown body + badges (reading view)
//   card     — title + badges + a 1–2 line digest (compact, legible)
//   tile     — title + status glyph + counts (far/overview, clean at distance)
export type CanvasViewMode = "document" | "card" | "tile";

// The global picker is the three manual modes (no auto-by-zoom — §22.1).
export type CanvasViewModeSetting = CanvasViewMode;

export interface CanvasCardStyleProps {
  style: CSSProperties;
  lod?: CanvasLod;
  viewMode?: CanvasViewMode;
  onMouseDown?: (e: React.MouseEvent) => void;
  dragging?: boolean;
}
