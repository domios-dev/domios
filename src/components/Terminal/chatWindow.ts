import type { TimelineItem } from "@codeterm/chat-core";

/** The UI renders at most 220 rows; the retained source bound lives with the
 * transcript reducer, which evicts the oldest rows under byte-range authority. */
export const CHAT_WINDOW_SIZE = 220;
export const CHAT_WINDOW_STEP = 160;

export function timelineItemKey(item: TimelineItem): string {
  return item.kind === "message" ? item.message.id : item.event.message.id;
}

export interface ChatWindowRange {
  start: number;
  end: number;
}

export interface ChatWindowProjection {
  items: TimelineItem[];
  readingItems: TimelineItem[];
}

export function chatWindowProjection(
  items: readonly TimelineItem[],
  anchorKey: string | null,
  cachedReadingItems: readonly TimelineItem[] = [],
  size: number = CHAT_WINDOW_SIZE,
): ChatWindowProjection {
  const tailRange = chatWindowRange(items, null, size);
  const tail = items.slice(tailRange.start, tailRange.end);
  if (anchorKey == null) return { items: tail, readingItems: [] };

  const anchorIndex = items.findIndex((item) => timelineItemKey(item) === anchorKey);
  const currentItems = new Map(items.map((item) => [timelineItemKey(item), item]));
  const readingItems = anchorIndex >= 0
    ? items.slice(Math.max(0, anchorIndex + 1 - size), anchorIndex + 1)
    : cachedReadingItems.map((item) => currentItems.get(timelineItemKey(item)) ?? item);
  return {
    items: readingItems,
    readingItems,
  };
}

/** Anchor `null` = follow the tail. An anchor names the LAST visible item, so
 *  appends never move an anchored window and prepends shift it intact. */
export function chatWindowRange(
  items: readonly TimelineItem[],
  anchorKey: string | null,
  size: number = CHAT_WINDOW_SIZE,
): ChatWindowRange {
  const len = items.length;
  if (anchorKey != null) {
    for (let i = len - 1; i >= 0; i -= 1) {
      if (timelineItemKey(items[i]) === anchorKey) {
        const end = i + 1;
        return { start: Math.max(0, end - size), end };
      }
    }
  }
  return { start: Math.max(0, len - size), end: len };
}

/** Slide the window toward older items by up to `step`, returning the new
 *  anchor key (unchanged when nothing older is loaded locally). */
export function slideChatWindowOlder(
  items: readonly TimelineItem[],
  anchorKey: string | null,
  size: number = CHAT_WINDOW_SIZE,
  step: number = CHAT_WINDOW_STEP,
): string | null {
  const { start, end } = chatWindowRange(items, anchorKey, size);
  if (start <= 0) return anchorKey;
  const newEnd = Math.max(1, end - Math.min(step, start));
  return timelineItemKey(items[newEnd - 1]);
}
