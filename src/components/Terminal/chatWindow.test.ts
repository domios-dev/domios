import type { TimelineItem } from "@codeterm/chat-core";
import {
  CHAT_WINDOW_SIZE,
  CHAT_WINDOW_STEP,
  chatWindowRange,
  chatWindowProjection,
  slideChatWindowOlder,
  timelineItemKey,
} from "./chatWindow";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

function items(count: number, offset = 0): TimelineItem[] {
  return Array.from({ length: count }, (_, i) => ({
    kind: "message" as const,
    message: { id: `m-${i + offset}`, type: "assistant" as const, content: `c${i + offset}` },
  }));
}

// --- follow mode tracks the tail and stays bounded ---
{
  const list = items(500);
  const range = chatWindowRange(list, null);
  assert(range.end === 500 && range.start === 500 - CHAT_WINDOW_SIZE, "follow window is the bounded tail");
  const small = chatWindowRange(items(10), null);
  assert(small.start === 0 && small.end === 10, "short transcript shows everything");
}

// --- an anchored reading window stays stable while projection always includes the live tail ---
{
  const list = items(500);
  const anchor = timelineItemKey(list[299]);
  const before = chatWindowRange(list, anchor);
  const appended = [...list, ...items(50, 500)];
  const after = chatWindowRange(appended, anchor);
  assert(before.start === after.start && before.end === after.end, "appends never move an anchored window");
  assert(after.end === 300, "anchor names the last visible item");
  const projection = chatWindowProjection(appended, anchor);
  assert(projection.items.length === CHAT_WINDOW_SIZE * 2, "detached projection keeps reading and tail windows bounded");
  assert(timelineItemKey(projection.items[219]) === anchor, "reading window remains intact");
  assert(timelineItemKey(projection.items.at(-1)!) === "m-549", "newest tail is always projected");

  const replaced = items(550, 1_000);
  const recovered = chatWindowProjection(replaced, anchor, projection.readingItems);
  assert(timelineItemKey(recovered.items[219]) === anchor, "vanished source anchor keeps the cached reading window");
  assert(timelineItemKey(recovered.items.at(-1)!) === "m-1549", "cached reading never blocks the current tail");
}

// --- overlapping reading and tail windows merge without duplicate rows ---
{
  const list = items(300);
  const projection = chatWindowProjection(list, timelineItemKey(list[199]));
  assert(projection.items.length === 300, "overlapping windows deduplicate rows");
  assert(timelineItemKey(projection.items[0]) === "m-0" && timelineItemKey(projection.items.at(-1)!) === "m-299", "merged projection includes history and newest tail");
}

// --- prepends shift an anchored window intact (same content) ---
{
  const list = items(400);
  const anchor = timelineItemKey(list[350]);
  const prepended = [...items(30, 1000), ...list];
  const range = chatWindowRange(prepended, anchor);
  assert(range.end === 30 + 351, "prepend shifts the anchored range by the inserted count");
  assert(timelineItemKey(prepended[range.end - 1]) === anchor, "anchored item stays the window's last item");
}

// --- sliding older reveals older items and clamps at the front ---
{
  const list = items(500);
  let anchor: string | null = null;
  anchor = slideChatWindowOlder(list, anchor);
  const range = chatWindowRange(list, anchor);
  assert(range.end === 500 - CHAT_WINDOW_STEP, "one slide moves back by the step");
  // Slide until clamped.
  for (let i = 0; i < 10; i += 1) anchor = slideChatWindowOlder(list, anchor);
  const clamped = chatWindowRange(list, anchor);
  assert(clamped.start === 0, "sliding clamps at the oldest loaded item");
  assert(clamped.end === CHAT_WINDOW_SIZE, "clamped window keeps full size");
  const same = slideChatWindowOlder(list, anchor);
  assert(same === anchor, "sliding at the front is a no-op");
}

// --- a vanished anchor falls back to following the tail ---
{
  const list = items(300);
  const range = chatWindowRange(list, "gone-id");
  assert(range.end === 300 && range.start === 80, "unknown anchor follows the tail");
}

console.log("chat window checks passed");
