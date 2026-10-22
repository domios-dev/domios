export function sidebarScrollAnchorLayout(
  desiredScrollTop: number,
  contentHeight: number,
  viewportHeight: number,
): { scrollTop: number; trailingSpace: number } {
  const scrollTop = Math.max(0, desiredScrollTop);
  const naturalMaximum = Math.max(0, contentHeight - viewportHeight);
  return { scrollTop, trailingSpace: Math.max(0, Math.ceil(scrollTop - naturalMaximum)) };
}
