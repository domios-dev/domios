import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { hasMeaningfulToolResult, isRenderableMessage } from "./MessageBubble.helpers";
import { normalizedTimelineMarker } from "./timelineMarkers";

export function isRenderableChatRow(message: NormalizedChatMessage): boolean {
  const marker = normalizedTimelineMarker(message);
  if (marker && marker.kind !== "queued") return true;

  if (message.type === "assistant" || message.type === "thinking" || message.type === "status") {
    return message.content.trim().length > 0;
  }

  if ((message.type === "system" || message.type === "local_command") && !message.content.trim()) {
    return message.systemSubtype === "turn_aborted" || message.systemSubtype === "turn_stop_reason"
      || message.systemSubtype === "api_error" || message.systemSubtype === "archived_session_unavailable"
      || message.systemSubtype === "model_unavailable" || message.systemSubtype === "account_transition";
  }

  if (message.type === "tool_flow" || message.type === "custom") return false;
  if (message.type === "tool_result") return hasMeaningfulToolResult(message.content);
  return isRenderableMessage(message);
}
