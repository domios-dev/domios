import type { NormalizedChatMessage } from "./types";

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
      hasStructuredValue(message.toolInput) ||
      Boolean(message.interactive?.length)
    );
  }

  if (message.type === "tool_result") {
    return hasText(message.content) || hasText(message.toolResult);
  }

  return true;
}

export function attachToolResults<T extends NormalizedChatMessage>(messages: T[]): T[] {
  const grouped: T[] = [];
  const toolIndex = new Map<string, number>();
  let lastUnresolvedToolIndex: number | null = null;

  for (const message of messages) {
    if (!isRenderableChatMessage(message)) {
      continue;
    }

    if (message.type === "tool_call") {
      if (message.toolId) {
        toolIndex.set(message.toolId, grouped.length);
      }
      lastUnresolvedToolIndex = grouped.length;
      grouped.push(message);
      continue;
    }

    if (message.type === "tool_result") {
      const index = message.toolId ? toolIndex.get(message.toolId) : lastUnresolvedToolIndex;
      if (index != null) {
        const existing = grouped[index];
        const resultText = message.toolResult || message.content;
        const toolResult = existing.toolResult
          ? `${existing.toolResult}\n${resultText}`
          : resultText;
        grouped[index] = { ...existing, toolResult };
        if (!message.toolId || message.toolId === existing.toolId) {
          lastUnresolvedToolIndex = null;
        }
        continue;
      }
    }

    grouped.push(message);
  }

  return grouped;
}
