import { useSyncExternalStore } from "react";

export type ChatRevealOutcome = "pending" | "revealed" | "missing";

export interface ChatRevealRequest {
  id: number;
  notificationId: string | null;
  tabId: string;
  messageId: string;
  outcome: ChatRevealOutcome;
}

let nextId = 1;
let current: ChatRevealRequest | null = null;
const listeners = new Set<() => void>();

function publish(next: ChatRevealRequest | null): void {
  current = next;
  for (const listener of listeners) listener();
}

/** One request at a time: a newer jump replaces an unsettled one. */
export function requestChatMessageReveal(
  tabId: string,
  messageId: string,
  notificationId: string | null = null,
): ChatRevealRequest {
  const request: ChatRevealRequest = { id: nextId++, notificationId, tabId, messageId, outcome: "pending" };
  publish(request);
  return request;
}

export function settleChatMessageReveal(id: number, outcome: Exclude<ChatRevealOutcome, "pending">): void {
  if (!current || current.id !== id || current.outcome !== "pending") return;
  publish({ ...current, outcome });
}

export function currentChatReveal(): ChatRevealRequest | null {
  return current;
}

export function clearChatReveal(): void {
  publish(null);
}

export function subscribeChatReveal(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useChatReveal(): ChatRevealRequest | null {
  return useSyncExternalStore(subscribeChatReveal, currentChatReveal, currentChatReveal);
}
