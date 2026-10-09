import type { NormalizedChatMessage } from "@codeterm/chat-core";

export function streamingMessageId(rows: readonly { msg: NormalizedChatMessage }[], active: boolean): string | null {
  const tail = rows[rows.length - 1]?.msg;
  return active && tail?.type === "assistant" ? tail.id : null;
}
