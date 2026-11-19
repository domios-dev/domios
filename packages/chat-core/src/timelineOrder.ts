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

/** Sorts by authoritative time. Untimed rows retain reducer source order because
 *  provider sequence ordinals can restart, and follow timed rows without a known time. */
export function orderTimelineItems<T extends TimelineOrderItem>(items: readonly T[]): T[] {
  return items.map((item, index) => ({ item, index, at: timestampMs(item.timestamp) }))
    .sort((left, right) => {
      if (left.at != null && right.at != null) {
        if (left.at !== right.at) return left.at - right.at;
        const source = compareSource(left.item, right.item);
        return source || left.index - right.index;
      }
      if (left.at != null && right.at == null) return -1;
      if (left.at == null && right.at != null) return 1;
      return left.index - right.index;
    })
    .map(({ item }) => item);
}
