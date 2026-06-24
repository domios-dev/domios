import type { CanvasViewMode, CanvasViewModeSetting } from "./cards";

// View mode is a MANUAL choice — zoom is camera-only and never switches modes
// (§22.1, kills the old auto-by-distance). Text-off forces the bodiless Tile
// (deliberate declutter, never an accidental empty box).
export function resolveGlobalMode(setting: CanvasViewModeSetting, showText: boolean): CanvasViewMode {
  if (!showText) return "tile";
  return setting;
}
