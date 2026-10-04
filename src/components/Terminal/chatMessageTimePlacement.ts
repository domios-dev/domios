import { timestampMs, type NormalizedChatMessage } from "@codeterm/chat-core";

export function agentMessageTimeIds(messages: NormalizedChatMessage[]): Set<string> {
  const timestampIds = new Set<string>();
  let turnHasTimestamp = false;
  let sourceSessionId: string | undefined;

  for (const message of messages) {
    if (message.sourceSessionId) {
      if (sourceSessionId && message.sourceSessionId !== sourceSessionId) {
        turnHasTimestamp = false;
      }
      sourceSessionId = message.sourceSessionId;
    }
    if (message.type === "user") {
      turnHasTimestamp = false;
    } else if (message.type === "assistant" && !turnHasTimestamp && timestampMs(message.timestamp) != null) {
      timestampIds.add(message.id);
      turnHasTimestamp = true;
    }
  }

  return timestampIds;
}
