import { useSyncExternalStore } from "react";
import { sessionChanges, type SessionChanges, type NormalizedChatMessage } from "@codeterm/chat-core";

const empty = sessionChanges([]);
const entries = new Map<string, SessionChanges>();
const authorities = new Map<string, string>();
const listeners = new Set<() => void>();
export const chatChangesStore = {
  bind(tabId: string, authority: string) {
    if (authorities.get(tabId) === authority) return;
    authorities.set(tabId, authority);
    entries.delete(tabId);
    for (const listener of listeners) listener();
  },
  set(tabId: string, messages: readonly NormalizedChatMessage[]) {
    entries.set(tabId, sessionChanges(messages));
    for (const listener of listeners) listener();
  },
  get(tabId: string) { return entries.get(tabId) ?? empty; },
  clear() { entries.clear(); authorities.clear(); for (const listener of listeners) listener(); },
  subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
};
export function useChatChanges(tabId: string) {
  return useSyncExternalStore(chatChangesStore.subscribe, () => chatChangesStore.get(tabId), () => empty);
}
