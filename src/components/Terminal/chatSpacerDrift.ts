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

export function chatSpacerAfterLayout({ base, drift, shift, absorb }: {
  base: number; drift: number; shift: number; absorb: boolean; viewportHeight: number; scrollTop: number;
}): { drift: number; scrollBy: number } {
  const owned = absorb && Number.isFinite(shift) ? shift : 0;
  return { drift: chatSpacerDriftAfterShift(drift, base, owned), scrollBy: 0 };
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
