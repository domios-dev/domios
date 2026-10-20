import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { hasMeaningfulToolResult, isRenderableMessage } from "./MessageBubble.helpers";
import { normalizedTimelineMarker } from "./timelineMarkers";

export function isRenderableChatRow(message: NormalizedChatMessage): boolean {
  const marker = normalizedTimelineMarker(message);
  if (marker && marker.kind !== "queued") return true;

  if (message.type === "assistant" || message.type === "thinking" || message.type === "status") {
    return message.content.trim().length > 0;
  }

  if (message.type === "tool_flow" || message.type === "custom") return false;
  if (message.type === "tool_result") return hasMeaningfulToolResult(message.content);
  return isRenderableMessage(message);
}
