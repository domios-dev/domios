import { timestampMs } from "./time";

/** Identity and ordering fields shared by transcript rows and timestamped host cards. */
export interface TimelineOrderItem {
  id: string;
  timestamp?: string | number | null;
  sourceMessageId?: string;
  sourceSessionId?: string;
  seq?: number;
}

function compareText(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function compareSource(left: TimelineOrderItem, right: TimelineOrderItem): number {
  const session = compareText(left.sourceSessionId ?? "", right.sourceSessionId ?? "");
  if (session) return session;
  if (left.seq != null && right.seq != null && left.seq !== right.seq) return left.seq - right.seq;
  const leftId = left.sourceMessageId ?? left.id;
  const rightId = right.sourceMessageId ?? right.id;
  return compareText(leftId, rightId);
}

/** Sort timestamped runs independently so untimed rows keep their source boundary. */
export function orderTimelineItems<T extends TimelineOrderItem>(items: readonly T[]): T[] {
  const ordered = items.map((item, index) => ({ item, index, at: timestampMs(item.timestamp) }));
  const result = [...items];
  let start = 0;
  while (start < ordered.length) {
    if (ordered[start].at == null) {
      start++;
      continue;
    }
    let end = start + 1;
    while (end < ordered.length && ordered[end].at != null) end++;
    const run = ordered.slice(start, end).sort((left, right) => {
      if (left.at !== right.at) return left.at! - right.at!;
      const source = compareSource(left.item, right.item);
      return source || left.index - right.index;
    });
    run.forEach(({ item }, offset) => { result[start + offset] = item; });
    start = end;
  }
  return result;
}
