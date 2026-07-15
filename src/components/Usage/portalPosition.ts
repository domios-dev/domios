export function clampPortalCenter(center: number, width: number, viewportWidth: number, margin = 8): number {
  const available = Math.max(0, viewportWidth - margin * 2);
  const effectiveWidth = Math.min(width, available);
  const half = effectiveWidth / 2;
  return Math.max(margin + half, Math.min(viewportWidth - margin - half, center));
}
