import type { ViewportTransform } from "@codeterm/canvas-camera";

export function canvasGrid(transform: ViewportTransform) {
  const baseSpacing = 24 * transform.scale;
  const spacing = baseSpacing * 2 ** Math.ceil(Math.log2(16 / baseSpacing));
  return {
    spacing,
    radius: Math.max(1, Math.min(1.5, transform.scale)),
    x: transform.x - spacing / 2,
    y: transform.y - spacing / 2,
  };
}
