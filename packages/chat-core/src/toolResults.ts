import type { NormalizedChatMessage } from "./types";
import { reduceToolEvents } from "./toolEvents";

function hasStructuredValue(value: unknown): boolean {
  if (value == null) return false;
  if (typeof value === "string") return value.trim().length > 0;
  if (Array.isArray(value)) return value.length > 0;
  if (typeof value === "object") return Object.keys(value).length > 0;
  return true;
}

function hasText(value: string | undefined): boolean {
  return typeof value === "string" && value.trim().length > 0;
}

export function isRenderableChatMessage(message: NormalizedChatMessage): boolean {
  if (message.type === "tool_call") {
    return (
      hasText(message.content) ||
      hasText(message.toolName) ||
      hasText(message.toolArgs) ||
      hasText(message.toolResult) ||
      hasText(message.toolSummary) ||
      hasText(message.toolDetail) ||
      Boolean(message.toolEdits?.length) ||
      hasStructuredValue(message.toolInput) ||
      Boolean(message.interactive?.length)
    );
  }

  if (message.type === "tool_result") {
    return hasText(message.content) || hasText(message.toolResult) || hasText(message.toolDetail);
  }

  return true;
}

export function attachToolResults<T extends NormalizedChatMessage>(messages: T[]): T[] {
  const compatibilityMessages = messages.map((message) =>
    message.type === "tool_result" && !message.toolDetail
      ? { ...message, toolDetail: message.toolResult || message.content }
      : message,
  );
  return reduceToolEvents(compatibilityMessages.filter(isRenderableChatMessage)).map((item) =>
    item.kind === "message" ? item.message : item.event.message,
  ) as T[];
}
