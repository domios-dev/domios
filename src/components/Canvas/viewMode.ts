import { DEFAULT_CANVAS_SETTINGS } from "./settings";
import type { CanvasViewMode, CanvasViewModeSetting } from "./cards";

// Auto-by-distance (§18.2): far → Tile, mid → Card, near → Document. Tile shares
// the LOD far band; Document kicks in around ~100% so the reading view appears at
// natural zoom (the LOD near band at 1.1 would keep Card past 100% — too late).
const AUTO_DOCUMENT_THRESHOLD = 0.85;

export function autoViewMode(scale: number): CanvasViewMode {
  const s = DEFAULT_CANVAS_SETTINGS;
  if (scale < s.zoomFarThreshold) return "tile";
  if (scale < AUTO_DOCUMENT_THRESHOLD) return "card";
  return "document";
}

// The board-wide resolved mode: text-off forces the bodiless Tile (deliberate
// declutter, never an accidental empty box); else honor the user's pick or auto.
export function resolveGlobalMode(
  setting: CanvasViewModeSetting,
  scale: number,
  showText: boolean,
): CanvasViewMode {
  if (!showText) return "tile";
  return setting === "auto" ? autoViewMode(scale) : setting;
}
