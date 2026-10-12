import { clampFloatingPosition } from "../shared/floatingPosition";

export function anchoredPanelPosition(
  rect: { bottom: number; right: number },
  viewportWidth: number,
): { top: number; right: number; maxWidth: number } {
  const gutter = 8;
  const minimumWidth = Math.min(240, Math.max(0, viewportWidth - gutter * 2));
  const { left } = clampFloatingPosition(
    rect.right - minimumWidth, 0, minimumWidth, 0, viewportWidth, 0, gutter,
  );
  const right = viewportWidth - left - minimumWidth;
  return {
    top: rect.bottom + 6,
    right,
    maxWidth: Math.max(0, viewportWidth - right - gutter),
  };
}
