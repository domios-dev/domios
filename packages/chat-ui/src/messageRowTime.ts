import { timestampMs, type ChatOrigin, type NormalizedChatMessage, type TimelineItem, type ToolEvent } from "@codeterm/chat-core";

function firstEventMessage(event: ToolEvent): NormalizedChatMessage {
  const first = event.stream?.members[0] ?? event.commandBatch?.[0] ?? event.editRun?.[0];
  return first ? firstEventMessage(first) : event.message;
}

/** Rows of a kind that shows a time; tool flows and notices never carry one. */
function isTimedMessageKind(message: NormalizedChatMessage): boolean {
  return message.type !== "tool_flow" && (message.type !== "custom" || message.timelineMarker?.kind === "context_compaction" || message.timelineMarker?.kind === "external_edit");
}

/** A transcript row with no known day, which a later dated row may share; a plain notice is not transcript. */
export function isUndatedTranscriptRow(message: NormalizedChatMessage, item?: TimelineItem): boolean {
  if (message.type === "custom" && !isTimedMessageKind(message)) return false;
  const timestamp = timestampMs((item?.kind === "tool" ? firstEventMessage(item.event) : message).timestamp);
  return timestamp == null || !Number.isFinite(new Date(timestamp).getTime());
}

export function messageRowTime(message: NormalizedChatMessage, item?: TimelineItem, origin?: ChatOrigin): { timestamp: number; side: "left" | "right" } | null {
  if (!isTimedMessageKind(message)) return null;
  const source = item?.kind === "tool" ? firstEventMessage(item.event) : message;
  const timestamp = timestampMs(source.timestamp);
  if (timestamp == null || !Number.isFinite(new Date(timestamp).getTime())) return null;
  const relayed = origin || message.inboundKind || message.fromTab || message.fromPlugin || message.fromMesh;
  return { timestamp, side: message.type === "user" && !relayed ? "right" : "left" };
}
