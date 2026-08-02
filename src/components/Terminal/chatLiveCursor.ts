export type ChatCursorDecision = "accept" | "stale" | "gap";

export function classifyChatCursor(
  currentOffset: number | null,
  startOffset: number | undefined,
  offset: number | undefined,
): ChatCursorDecision {
  if (currentOffset == null || startOffset == null || offset == null) return "accept";
  if (offset <= currentOffset) return "stale";
  return startOffset > currentOffset ? "gap" : "accept";
}

export function advanceChatCursor(currentOffset: number | null, offset: number | undefined): number | null {
  if (offset == null) return currentOffset;
  return currentOffset == null ? offset : Math.max(currentOffset, offset);
}
