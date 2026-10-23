export function sidebarScrollAnchorLayout(
  desiredScrollTop: number,
  contentHeight: number,
  viewportHeight: number,
): { scrollTop: number; trailingSpace: number } {
  const scrollTop = Math.max(0, desiredScrollTop);
  const naturalMaximum = Math.max(0, contentHeight - viewportHeight);
  return { scrollTop, trailingSpace: Math.max(0, Math.ceil(scrollTop - naturalMaximum)) };
}

/// Trailing space a user scroll may keep: it only shrinks, so scrolling never writes scrollTop back.
export function sidebarTrailingSpaceAfterScroll(
  scrollTop: number,
  contentHeight: number,
  viewportHeight: number,
  currentTrailingSpace: number,
): number {
  return Math.min(currentTrailingSpace, sidebarScrollAnchorLayout(scrollTop, contentHeight, viewportHeight).trailingSpace);
}

/// Sub-pixel anchor drift is layout rounding, not a content shift; correcting it would fight the scroller.
export function sidebarAnchorShift(currentY: number, capturedY: number): number {
  const shift = currentY - capturedY;
  return Math.abs(shift) < 0.5 ? 0 : shift;
}
