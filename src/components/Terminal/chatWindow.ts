import type { TimelineItem } from "@codeterm/chat-core";

export const CHAT_WINDOW_SIZE = 220;
export const CHAT_WINDOW_STEP = 160;

export function timelineItemKey(item: TimelineItem): string {
  return item.kind === "message" ? item.message.id : item.event.id;
}

export interface ChatWindowRange {
  start: number;
  end: number;
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
