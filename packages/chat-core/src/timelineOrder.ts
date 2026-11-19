import { timestampMs } from "./time";

/** Identity and ordering fields shared by transcript rows and timestamped host cards. */
export interface TimelineOrderItem {
  id: string;
  timestamp?: string | number | null;
  sourceMessageId?: string;
  sourceSessionId?: string;
  seq?: number;
}

interface IndexedItem<T extends TimelineOrderItem> {
  item: T;
  index: number;
  at: number | null;
}

function orderTimestampRun<T extends TimelineOrderItem>(items: IndexedItem<T>[]): IndexedItem<T>[] {
  const ordered = items.sort((left, right) => left.at! - right.at! || left.index - right.index);
  let timeStart = 0;
  while (timeStart < ordered.length) {
    let timeEnd = timeStart + 1;
    while (timeEnd < ordered.length && ordered[timeEnd].at === ordered[timeStart].at) timeEnd++;

    let sequenceStart = timeStart;
    while (sequenceStart < timeEnd) {
      const first = ordered[sequenceStart];
      if (first.item.sourceSessionId == null || first.item.seq == null) {
        sequenceStart++;
        continue;
      }
      let sequenceEnd = sequenceStart + 1;
      while (sequenceEnd < timeEnd) {
        const next = ordered[sequenceEnd];
        if (next.item.sourceSessionId !== first.item.sourceSessionId || next.item.seq == null) break;
        sequenceEnd++;
      }
      const sequenceRun = ordered.slice(sequenceStart, sequenceEnd).sort((left, right) =>
        left.item.seq! - right.item.seq! || left.index - right.index,
      );
      sequenceRun.forEach((item, offset) => { ordered[sequenceStart + offset] = item; });
      sequenceStart = sequenceEnd;
    }
    timeStart = timeEnd;
  }
  return ordered;
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
    const run = orderTimestampRun(ordered.slice(start, end));
    run.forEach(({ item }, offset) => { result[start + offset] = item; });
    start = end;
  }
  return result;
}
