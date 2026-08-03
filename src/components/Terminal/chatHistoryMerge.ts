import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";

/** Stamp arrival ordinals and the owning session on a batch. `clientOrder` is
 *  the only signal that places an optimistic row against rows that arrived
 *  after it; it never participates in transcript identity. */
export function withPreservedClientOrder(
  messages: ChatMessage[],
  previous: ChatMessage[],
  nextOrder: () => number,
  sourceSessionId: string | null = null,
): ChatMessage[] {
  const orders = new Map(previous.map((message) => [message.id, message.clientOrder]));
  return messages.map((message) => {
    const clientOrder = message.clientOrder ?? orders.get(message.id) ?? nextOrder();
    const sessionId = message.sourceSessionId ?? sourceSessionId ?? undefined;
    if (message.clientOrder === clientOrder && message.sourceSessionId === sessionId) return message;
    return { ...message, clientOrder, sourceSessionId: sessionId };
  });
}
