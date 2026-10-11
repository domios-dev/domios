export const HIGHLIGHT_LINE_LIMIT = 200;
export const HIGHLIGHT_BYTE_LIMIT = 16 * 1024;
export const CHAT_HIGHLIGHT_SPAN_LIMIT = 5000;
export const ROW_HIGHLIGHT_SPAN_LIMIT = 2000;

export interface HighlightReservation { rowId: string; spans: number }
export type HighlightReservations = ReadonlyMap<string, HighlightReservation>;

export function highlightSourceEligible(source: string, explicit = false): boolean {
  return explicit || (source.replace(/\n$/, "").split("\n").length <= HIGHLIGHT_LINE_LIMIT && new TextEncoder().encode(source).byteLength <= HIGHLIGHT_BYTE_LIMIT);
}

export function reserveHighlightSpans(current: HighlightReservations, key: string, rowId: string, spans: number): HighlightReservations {
  let chatUsed = 0;
  let rowUsed = 0;
  for (const [id, reservation] of current) {
    if (id === key) continue;
    chatUsed += reservation.spans;
    if (reservation.rowId === rowId) rowUsed += reservation.spans;
  }
  const allowed = Number.isSafeInteger(spans) && spans > 0
    && chatUsed + spans <= CHAT_HIGHLIGHT_SPAN_LIMIT && rowUsed + spans <= ROW_HIGHLIGHT_SPAN_LIMIT;
  const previous = current.get(key);
  if ((!allowed && previous == null) || (allowed && previous?.rowId === rowId && previous.spans === spans)) return current;
  const next = new Map(current);
  if (allowed) next.set(key, { rowId, spans });
  else next.delete(key);
  return next;
}

export interface HighlightNode { type: string; tagName?: unknown; children?: readonly HighlightNode[] }

export function highlightSpanCount(nodes: readonly HighlightNode[]): number {
  let count = 0;
  for (const node of nodes) {
    if (node.tagName === "span") count += 1;
    if (node.children) count += highlightSpanCount(node.children);
  }
  return count;
}
