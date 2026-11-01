/// Content that fits has no scroll range, and its height reads as the viewport's (scrollHeight floors at
/// clientHeight), so a retained offset would be an unreachable write the browser clamps.
export function sidebarScrollAnchorLayout(
  desiredScrollTop: number,
  contentHeight: number,
  viewportHeight: number,
): { scrollTop: number; trailingSpace: number } {
  if (contentHeight <= viewportHeight) return { scrollTop: 0, trailingSpace: 0 };
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

interface SidebarAnchorPosition {
  y: number;
  scrollTop: number;
  maximum: number;
}

export function sidebarAnchorScrollTarget(
  captured: SidebarAnchorPosition,
  current: SidebarAnchorPosition,
  nativeOwned: boolean,
): number | null {
  if (nativeOwned) return null;
  const clampedByLayout = current.maximum < captured.maximum
    && captured.scrollTop > current.maximum && current.scrollTop === current.maximum;
  const scrollDelta = clampedByLayout ? 0 : current.scrollTop - captured.scrollTop;
  const shift = sidebarAnchorShift(current.y + scrollDelta, captured.y);
  return shift === 0 ? null : current.scrollTop + shift;
}

export interface SidebarScrollGesture {
  phase: "idle" | "touch" | "momentum";
  moved: boolean;
}

export type SidebarScrollGestureEvent = "touchstart" | "touchmove" | "touchend" | "touchcancel" | "scrollend" | "press";

export function sidebarScrollGesture(state: SidebarScrollGesture, event: SidebarScrollGestureEvent): SidebarScrollGesture {
  switch (event) {
    case "touchstart": return { phase: "touch", moved: false };
    case "touchmove": return { phase: "touch", moved: true };
    case "touchend": return { phase: state.moved ? "momentum" : "idle", moved: state.moved };
    case "touchcancel": return { phase: "momentum", moved: true };
    case "scrollend": return state.phase === "touch" ? state : { phase: "idle", moved: false };
    case "press": return { phase: "idle", moved: false };
  }
}
