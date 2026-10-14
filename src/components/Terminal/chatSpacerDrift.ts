/** A row's offset in the scroller's content; an unchanged top means content above it kept its height. */
export interface ChatShiftAnchor {
  id: string;
  top: number;
}

export interface ChatShiftContext {
  nativeAnchoring: boolean;
  pinned: boolean;
  userScrolling: boolean;
}

// WebKit has no CSS scroll anchoring, and a scrollTop write mid-gesture fights iOS momentum, so the spacer absorbs the shift.
export function shouldAbsorbChatShift({ nativeAnchoring, pinned, userScrolling }: ChatShiftContext): boolean {
  return !nativeAnchoring && !pinned && userScrolling;
}

/** Drift added to the top spacer so the anchor keeps its content offset; the spacer never shrinks below zero. */
export function chatSpacerDriftAfterShift(drift: number, base: number, shift: number): number {
  if (!Number.isFinite(shift) || shift === 0) return drift;
  return Math.max(-Math.max(0, base), drift - shift);
}

export function chatSpacerHeight(base: number, drift: number): number {
  return Math.max(0, base + drift);
}

/** Removing the drift moves everything below the spacer, so an idle viewport scrolls by the same amount to stay put. */
export function chatSpacerFold(base: number, drift: number): { drift: 0; scrollBy: number } {
  return { drift: 0, scrollBy: chatSpacerHeight(base, 0) - chatSpacerHeight(base, drift) };
}

/** Near the transcript top the spacer runs out of room to absorb, so the drift folds even mid-gesture. */
export function shouldFoldChatSpacerDrift(base: number, drift: number, viewportHeight: number): boolean {
  return drift !== 0 && (base <= 0 || base + drift < 2 * viewportHeight);
}

/**
 * Resolves one layout pass: the spacer absorbs an owned shift, or folds back to `base` with the scroll delta that keeps
 * the anchor still. The fold decides on the unclamped drift so a shift larger than the spacer never leaks to the screen.
 * A fold that would scroll above zero mid-gesture keeps the remainder as space above the first row until the gesture ends.
 */
export function chatSpacerAfterLayout({ base, drift, shift, absorb, viewportHeight, scrollTop }: {
  base: number; drift: number; shift: number; absorb: boolean; viewportHeight: number; scrollTop: number;
}): { drift: number; scrollBy: number } {
  const owned = absorb && Number.isFinite(shift) ? shift : 0;
  const wanted = drift - owned;
  if (shouldFoldChatSpacerDrift(base, wanted, viewportHeight)) {
    const scrollBy = chatSpacerHeight(base, 0) - chatSpacerHeight(base, drift) + owned;
    const overshoot = -(scrollTop + scrollBy);
    return absorb && overshoot > 0 ? { drift: overshoot, scrollBy: 0 - scrollTop } : { drift: 0, scrollBy };
  }
  return { drift: chatSpacerDriftAfterShift(drift, base, owned), scrollBy: 0 };
}

export function chatAnchorShift(anchor: ChatShiftAnchor | null, top: number | null): number {
  return anchor == null || top == null ? 0 : top - anchor.top;
}

/** The first row that still reaches into the viewport; content above it is what may shift the view. */
export function chatShiftAnchorAt(rows: readonly ChatShiftAnchor[], heights: readonly number[], scrollTop: number): ChatShiftAnchor | null {
  for (let index = 0; index < rows.length; index++) {
    if (rows[index].top + heights[index] > scrollTop) return rows[index];
  }
  return rows[rows.length - 1] ?? null;
}
