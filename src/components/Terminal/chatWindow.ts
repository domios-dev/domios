import type { NormalizedChatMessage, TimelineItem } from "@codeterm/chat-core";

/** The UI renders at most 220 rows and retains the newest 440 source messages.
 * Capacity pressure always removes the oldest source rows first. */
export const CHAT_WINDOW_SIZE = 220;
export const CHAT_WINDOW_STEP = 160;
export const CHAT_RETAINED_MESSAGE_LIMIT = CHAT_WINDOW_SIZE * 2;

export interface ChatPredecessorRange {
  startOffset: number;
  endOffset: number;
  seekBeforeOffset: number;
  pageAnchorId: string | null;
  boundaryVerified: boolean;
}

export function chatPredecessorRange(startOffset: number, endOffset: number): ChatPredecessorRange | null {
  if (endOffset <= 0) return null;
  return { startOffset, endOffset, seekBeforeOffset: endOffset, pageAnchorId: null, boundaryVerified: false };
}

export interface RetainedChatHistory {
  messages: NormalizedChatMessage[];
  omittedOlder: number;
  predecessorRange: ChatPredecessorRange | null;
}

export interface CachedChatHistoryWindow {
  history: RetainedChatHistory;
  anchorKey: string | null;
}

const MAX_CACHED_HISTORY_WINDOWS = 8;
const cachedHistoryWindows = new Map<string, CachedChatHistoryWindow>();

export function getCachedChatHistoryWindow(key: string): CachedChatHistoryWindow | null {
  const cached = cachedHistoryWindows.get(key);
  if (!cached) return null;
  cachedHistoryWindows.delete(key);
  cachedHistoryWindows.set(key, cached);
  return cached;
}

export function setCachedChatHistoryWindow(key: string, value: CachedChatHistoryWindow | null): void {
  cachedHistoryWindows.delete(key);
  if (value) cachedHistoryWindows.set(key, value);
  while (cachedHistoryWindows.size > MAX_CACHED_HISTORY_WINDOWS) {
    const oldest = cachedHistoryWindows.keys().next().value;
    if (oldest == null) break;
    cachedHistoryWindows.delete(oldest);
  }
}

export function timelineItemKey(item: TimelineItem): string {
  return item.kind === "message" ? item.message.id : item.event.message.id;
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

export function retainChatSnapshotTail(
  messages: readonly NormalizedChatMessage[],
  limit: number = CHAT_RETAINED_MESSAGE_LIMIT,
  predecessorRange: ChatPredecessorRange | null = null,
): RetainedChatHistory {
  const start = Math.max(0, messages.length - limit);
  return {
    messages: messages.slice(start),
    omittedOlder: start,
    predecessorRange,
  };
}

export function mergeRetainedChatLive(
  retained: RetainedChatHistory,
  merged: readonly NormalizedChatMessage[],
  limit: number = CHAT_RETAINED_MESSAGE_LIMIT,
): RetainedChatHistory {
  const start = Math.max(0, merged.length - limit);
  return {
    messages: merged.slice(start),
    omittedOlder: retained.omittedOlder + start,
    predecessorRange: retained.predecessorRange,
  };
}

export function prependRetainedChatHistory(
  retained: RetainedChatHistory,
  older: readonly NormalizedChatMessage[],
  step: number = CHAT_WINDOW_STEP,
): RetainedChatHistory {
  const retainedIds = new Set(retained.messages.map((message) => message.id));
  const uniqueOlder = older.filter((message) => !retainedIds.has(message.id)).slice(-step);
  return {
    messages: [...uniqueOlder, ...retained.messages],
    omittedOlder: Math.max(0, retained.omittedOlder - uniqueOlder.length),
    predecessorRange: retained.predecessorRange,
  };
}

export function refillRetainedChatHistory(
  retained: RetainedChatHistory,
  snapshot: readonly NormalizedChatMessage[],
  limit: number = CHAT_RETAINED_MESSAGE_LIMIT,
  step: number = CHAT_WINDOW_STEP,
): RetainedChatHistory {
  const oldestId = retained.messages[0]?.id;
  if (!oldestId) return retainChatSnapshotTail(snapshot, limit);
  const oldestIndex = snapshot.findIndex((message) => message.id === oldestId);
  if (oldestIndex < 0) return retained;
  if (oldestIndex === 0) return { ...retained, omittedOlder: 0 };
  const older = snapshot.slice(Math.max(0, oldestIndex - step), oldestIndex);
  const refilled = prependRetainedChatHistory(retained, older, step);
  return {
    ...refilled,
    omittedOlder: Math.max(0, oldestIndex - older.length),
  };
}

export interface ChatPredecessorSeekResult {
  history: RetainedChatHistory;
  matchedBoundary: boolean;
  prepended: boolean;
}

export function seekRetainedChatPredecessor(
  retained: RetainedChatHistory,
  page: readonly NormalizedChatMessage[],
  pageStartOffset: number,
  step: number = CHAT_WINDOW_STEP,
): ChatPredecessorSeekResult {
  const range = retained.predecessorRange;
  const boundaryId = range?.pageAnchorId ?? retained.messages[0]?.id;
  if (!range || !boundaryId) {
    return { history: retained, matchedBoundary: false, prepended: false };
  }
  const boundaryIndex = range.boundaryVerified && range.pageAnchorId == null
    ? page.length
    : page.findIndex((message) => message.id === boundaryId);
  if (boundaryIndex < 0) {
    const progressed = pageStartOffset >= 0 && pageStartOffset < range.seekBeforeOffset;
    return {
      history: progressed
        ? {
            ...retained,
            predecessorRange: {
              ...range,
              seekBeforeOffset: pageStartOffset,
              pageAnchorId: null,
              boundaryVerified: false,
            },
          }
        : retained,
      matchedBoundary: false,
      prepended: false,
    };
  }
  const available = Math.min(step, retained.omittedOlder, boundaryIndex);
  if (available <= 0) {
    return {
      history: {
        ...retained,
        predecessorRange: {
          ...range,
          seekBeforeOffset: pageStartOffset,
          pageAnchorId: null,
          boundaryVerified: true,
        },
      },
      matchedBoundary: true,
      prepended: false,
    };
  }
  const pageWindowStart = boundaryIndex - available;
  const older = page.slice(pageWindowStart, boundaryIndex);
  const history = prependRetainedChatHistory(retained, older, step);
  return {
    history: {
      ...history,
      predecessorRange: {
        ...range,
        seekBeforeOffset: pageWindowStart > 0 ? range.seekBeforeOffset : pageStartOffset,
        pageAnchorId: pageWindowStart > 0 ? older[0]?.id ?? null : null,
        boundaryVerified: true,
      },
    },
    matchedBoundary: true,
    prepended: older.length > 0,
  };
}
