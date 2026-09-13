import type { NormalizedChatMessage } from "./types";
import type { TimelineItem, ToolEvent } from "./toolEvents";

function timelineItemId(item: TimelineItem): string {
  return item.kind === "message" ? `m:${item.message.id}` : `t:${item.event.id}`;
}

function messagesEquivalent(a: NormalizedChatMessage, b: NormalizedChatMessage): boolean {
  if (a === b) return true;
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
  for (const key of keys) {
    if (a[key as keyof NormalizedChatMessage] !== b[key as keyof NormalizedChatMessage]) return false;
  }
  return true;
}

function toolEventsEquivalent(a: ToolEvent, b: ToolEvent): boolean {
  if (a === b) return true;
  if (
    a.id !== b.id ||
    a.toolId !== b.toolId ||
    a.phase !== b.phase ||
    a.status !== b.status ||
    a.kind !== b.kind ||
    a.summary !== b.summary ||
    a.description !== b.description ||
    a.title !== b.title ||
    a.flowId !== b.flowId ||
    a.detail !== b.detail ||
    a.result !== b.result ||
    a.error !== b.error ||
    a.edits !== b.edits
  ) {
    return false;
  }
  if (!messagesEquivalent(a.message, b.message)) return false;
  const streamA = a.stream;
  const streamB = b.stream;
  if (!streamA || !streamB) return streamA === streamB;
  if (streamA.flowId !== streamB.flowId || streamA.active !== streamB.active || streamA.members.length !== streamB.members.length) return false;
  return streamA.members.every((member, index) => toolEventsEquivalent(member, streamB.members[index]));
}

/**
 * Reuses items from a previous timeline when their content is unchanged, so
 * downstream React.memo consumers keep identity across streamed chunks. Both
 * arms must compare by content: tool events are re-materialized on every reduce
 * pass, and messages are re-materialized by the context-epoch projection.
 */
export function stabilizeTimelineItems(
  previous: TimelineItem[] | undefined,
  next: TimelineItem[],
): TimelineItem[] {
  if (!previous?.length || !next.length) return next;
  const previousById = new Map<string, TimelineItem>();
  for (const item of previous) previousById.set(timelineItemId(item), item);
  return next.map((item) => {
    const prior = previousById.get(timelineItemId(item));
    if (!prior || prior.kind !== item.kind) return item;
    if (item.kind === "message" && prior.kind === "message") {
      return messagesEquivalent(prior.message, item.message) ? prior : item;
    }
    if (item.kind === "tool" && prior.kind === "tool") {
      return toolEventsEquivalent(prior.event, item.event) ? prior : item;
    }
    return item;
  });
}
