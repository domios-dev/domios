export interface FloatingPosition {
  top: number;
  left: number;
  maxHeight: number;
}

export function clampFloatingPosition(
  x: number,
  y: number,
  width: number,
  height: number,
  viewportWidth: number,
  viewportHeight: number,
  margin = 4,
): FloatingPosition {
  const fittedWidth = Math.min(width, Math.max(0, viewportWidth - margin * 2));
  const fittedHeight = Math.min(height, Math.max(0, viewportHeight - margin * 2));
  const left = Math.max(margin, Math.min(x, viewportWidth - fittedWidth - margin));
  const top = Math.max(margin, Math.min(y, viewportHeight - fittedHeight - margin));
  return { top, left, maxHeight: Math.max(0, viewportHeight - top - margin) };
}
