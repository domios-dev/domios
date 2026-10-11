import { timestampMs, type ChatOrigin, type NormalizedChatMessage, type TimelineItem, type ToolEvent } from "@codeterm/chat-core";

function firstEventMessage(event: ToolEvent): NormalizedChatMessage {
  const first = event.stream?.members[0] ?? event.commandBatch?.[0];
  return first ? firstEventMessage(first) : event.message;
}

export function messageRowTime(message: NormalizedChatMessage, item?: TimelineItem, origin?: ChatOrigin): { timestamp: number; side: "left" | "right" } | null {
  if (message.type === "tool_flow" || (message.type === "custom" && message.timelineMarker?.kind !== "context_compaction" && message.timelineMarker?.kind !== "external_edit")) return null;
  const source = item?.kind === "tool" ? firstEventMessage(item.event) : message;
  const timestamp = timestampMs(source.timestamp);
  if (timestamp == null || !Number.isFinite(new Date(timestamp).getTime())) return null;
  const relayed = origin || message.inboundKind || message.fromTab || message.fromPlugin || message.fromMesh;
  return { timestamp, side: message.type === "user" && !relayed ? "right" : "left" };
}
