import { reduceToolEvents, type NormalizedChatMessage, type TimelineItem } from "@codeterm/chat-core";
import { enrichTimelineItems, type EnrichmentCache } from "./chatEnrichment";
import { chatWindowRows } from "./chatWindowRowsModel";
import { chatWindowRange, timelineItemKey } from "./chatWindow";

export const CHAT_PAGE_SIZE = 100;
export const CHAT_READING_SIZE = 300;

export function conversationEntries(messages: NormalizedChatMessage[], cache: EnrichmentCache = new WeakMap()) {
  return chatWindowRows(enrichTimelineItems(reduceToolEvents(messages), cache));
}

export function earlierMessageLabel(loaded: number, canBackfill: boolean): string {
  const count = Math.min(CHAT_PAGE_SIZE, loaded);
  return canBackfill && count < CHAT_PAGE_SIZE
    ? `Load previous messages (up to ${CHAT_PAGE_SIZE})`
    : `Load previous ${count} ${count === 1 ? "message" : "messages"}`;
}

export function revealEarlierMessages(items: readonly TimelineItem[], anchor: string | null, size: number) {
  const range = chatWindowRange(items, anchor, size);
  const count = Math.min(CHAT_PAGE_SIZE, range.start);
  const previousSize = range.end - range.start;
  const nextSize = Math.min(CHAT_READING_SIZE, previousSize + count);
  const end = range.end - (previousSize + count - nextSize);
  return { anchor: end > 0 ? timelineItemKey(items[end - 1]) : anchor, size: nextSize, count };
}
