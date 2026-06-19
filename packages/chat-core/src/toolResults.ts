import type { NormalizedChatMessage } from "./types";

export function attachToolResults<T extends NormalizedChatMessage>(messages: T[]): T[] {
  const grouped: T[] = [];
  const toolIndex = new Map<string, number>();
  let lastUnresolvedToolIndex: number | null = null;

  for (const message of messages) {
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
