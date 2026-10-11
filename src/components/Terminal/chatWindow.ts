import { chatRevealIndex } from "./chatAnchorModel";
import type { TimelineItem } from "@codeterm/chat-core";

/** Logical history projection; the mounted viewport is bounded independently. */
export const CHAT_WINDOW_SIZE = 220;
export const CHAT_WINDOW_STEP = 160;

export function timelineItemKey(item: TimelineItem): string {
  return item.kind === "message" ? item.message.id : item.event.message.id;
}

export function chatMessageAnchor(items: readonly TimelineItem[], sourceMessageId: string): string | null {
  const index = chatRevealIndex(items.map((candidate) => candidate.kind === "message" ? candidate.message : candidate.event.message), sourceMessageId);
  const item = items[index];
  return item ? timelineItemKey(item) : null;
}

export interface ChatWindowRange {
  start: number;
  end: number;
}

export interface ChatWindowProjection {
  items: TimelineItem[];
  newerCount: number;
}

export function chatWindowProjection(
  items: readonly TimelineItem[],
  anchorKey: string | null,
  size: number = CHAT_WINDOW_SIZE,
): ChatWindowProjection {
  const tailRange = chatWindowRange(items, null, size);
  const tail = items.slice(tailRange.start, tailRange.end);
  if (anchorKey == null) return { items: tail, newerCount: 0 };

  const anchorIndex = items.findIndex((item) => timelineItemKey(item) === anchorKey);
  if (anchorIndex < 0) return { items: tail, newerCount: 0 };
  return { items: items.slice(Math.max(0, anchorIndex + 1 - size), anchorIndex + 1), newerCount: items.length - anchorIndex - 1 };
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

export interface ChatWindowAnchorIdentity {
  sourceId: string;
  correlationId?: string;
}

function itemMessage(item: TimelineItem) {
  return item.kind === "message" ? item.message : item.event.message;
}

export function chatWindowAnchorIdentity(
  items: readonly TimelineItem[],
  anchorKey: string | null,
): ChatWindowAnchorIdentity | null {
  if (anchorKey == null) return null;
  const item = items.find((candidate) => timelineItemKey(candidate) === anchorKey);
  if (!item) return null;
  const message = itemMessage(item);
  return { sourceId: message.sourceMessageId ?? message.id, correlationId: message.correlationId };
}

/** The current key of the anchor's source row after a range re-key or an
 *  optimistic-to-committed replacement; null when that row is gone. */
export function rekeyedChatWindowAnchor(
  items: readonly TimelineItem[],
  identity: ChatWindowAnchorIdentity,
): string | null {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const message = itemMessage(items[index]);
    if (
      (message.sourceMessageId ?? message.id) === identity.sourceId
      || (identity.correlationId != null && message.correlationId === identity.correlationId)
    ) {
      return timelineItemKey(items[index]);
    }
  }
  return null;
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

export function chatReadingWindowAnchor(
  items: readonly TimelineItem[],
  anchorKey: string | null,
  followTail: boolean,
  previousTail: ChatWindowAnchorIdentity | null,
): string | null {
  if (anchorKey != null || followTail || previousTail == null) return anchorKey;
  return rekeyedChatWindowAnchor(items, previousTail);
}
