import { chatVisibleAnchorIndex } from "./chatAnchorModel";

/** `top` frames the whole row; `contentTop` is the message body, which a day separator above it never shifts. */
export interface ChatShiftAnchor {
  id: string;
  top: number;
  contentTop?: number;
}

export interface ChatShiftContext {
  pinned: boolean;
  userScrolling: boolean;
}

// The spacer owns layout compensation during a gesture; native anchoring is disabled on every engine.
export function shouldAbsorbChatShift({ pinned, userScrolling }: ChatShiftContext): boolean {
  return !pinned && userScrolling;
}

/** Drift added to the top spacer so the anchor keeps its content offset; the spacer never shrinks below zero. */
export function chatSpacerDriftAfterShift(drift: number, _base: number, shift: number): number {
  if (!Number.isFinite(shift) || shift === 0) return drift;
  return drift - shift;
}

export function chatSpacerHeight(base: number, drift: number): number {
  return Math.max(0, base + drift);
}

export function chatSpacerTranslation(base: number, drift: number): number {
  return Math.min(0, base + drift);
}

export function chatLayoutViewportTop(scrollTop: number, origin: number, translation: number): number {
  return scrollTop - origin - translation;
}

/** Removing the drift moves everything below the spacer, so an idle viewport scrolls by the same amount to stay put. */
export function chatSpacerFold(_base: number, drift: number): { drift: 0; scrollBy: number } {
  return { drift: 0, scrollBy: drift === 0 ? 0 : -drift };
}

/** When the mounted range starts at row 0 (`base === 0`), positive drift above row 0 must never enter the viewport:
 *  at `scrollTop <= prefix` the top spacer is 0, and above `prefix` it stays off-screen above `scrollTop`. */
export function chatSpacerDriftForViewport({ base, drift, scrollTop, prefix = 0 }: {
  base: number; drift: number; scrollTop: number; prefix?: number;
}): number {
  if (base > 0 || drift <= 0 || !Number.isFinite(scrollTop)) return drift;
  const safePrefix = Number.isFinite(prefix) ? prefix : 0;
  return Math.min(drift, Math.max(0, scrollTop - safePrefix));
}

export function chatSpacerAfterLayout({ base, drift, shift, absorb, scrollTop, prefix = 0 }: {
  base: number; drift: number; shift: number; absorb: boolean; viewportHeight: number; scrollTop: number; prefix?: number;
}): { drift: number; scrollBy: number } {
  const owned = absorb && Number.isFinite(shift) ? shift : 0;
  const next = chatSpacerDriftAfterShift(drift, base, owned);
  return { drift: chatSpacerDriftForViewport({ base, drift: next, scrollTop, prefix }), scrollBy: 0 };
}

function shiftTop(row: ChatShiftAnchor): number {
  return row.contentTop ?? row.top;
}

export function chatAnchorShift(anchor: ChatShiftAnchor | null, measured: ChatShiftAnchor | null | undefined): number {
  return anchor == null || measured == null ? 0 : shiftTop(measured) - shiftTop(anchor);
}

/** Rows above the shift anchor whose measured height differs from the last height their resize notification recorded.
 *  A notification can arrive a frame late (a resize observer loop defers it); taking a new anchor before it lands would
 *  fold the growth into the baseline and the shift would never be absorbed. */
export function unrecordedRowsAbove(rows: readonly ChatShiftAnchor[], heights: readonly number[], anchorIndex: number,
  recorded: ReadonlyMap<string, number>): number[] {
  const pending: number[] = [];
  for (let index = 0; index < Math.min(anchorIndex, rows.length); index++) {
    const known = recorded.get(rows[index].id);
    if (known != null && Math.abs(known - heights[index]) > 0.5) pending.push(index);
  }
  return pending;
}

/** The first row that still reaches into the viewport; content above it is what may shift the view. */
export function chatShiftAnchorAt(rows: readonly ChatShiftAnchor[], heights: readonly number[], scrollTop: number): ChatShiftAnchor | null {
  return rows[chatVisibleAnchorIndex(rows.map((row, index) => ({ ...row, height: heights[index] })), scrollTop)] ?? null;
}
