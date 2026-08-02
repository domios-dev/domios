import { mergeById, type NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";

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

export function mergeSnapshotWindow(
  previous: ChatMessage[],
  snapshot: ChatMessage[],
  nextOrder: () => number,
  sourceSessionId: string | null,
): ChatMessage[] {
  const freshSnapshot = withPreservedClientOrder(snapshot, previous, nextOrder, sourceSessionId);
  if (freshSnapshot.some((message) => message.systemSubtype === "context_epoch_boundary")) {
    const currentSessionDeltas = previous.filter(
      (message) => message.sourceSessionId === sourceSessionId,
    );
    return mergeById(freshSnapshot, currentSessionDeltas);
  }
  return mergeById(previous, freshSnapshot);
}

export function replaceSnapshotWindow(
  previous: ChatMessage[],
  snapshot: ChatMessage[],
  requestOrder: number,
  nextOrder: () => number,
  sourceSessionId: string | null,
): ChatMessage[] {
  const liveSinceRequest = previous.filter(
    (message) => message.clientOrder != null && message.clientOrder >= requestOrder,
  );
  const freshSnapshot = withPreservedClientOrder(snapshot, previous, nextOrder, sourceSessionId);
  return mergeById(freshSnapshot, liveSinceRequest);
}
