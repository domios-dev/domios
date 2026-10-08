import { useSyncExternalStore } from "react";
import type { ChatDocument } from "./chatDocuments";
const entries = new Map<string, readonly ChatDocument[]>();
const listeners = new Set<() => void>();
const empty: readonly ChatDocument[] = [];
export const chatDocumentsStore = {
  set(tabId: string, documents: readonly ChatDocument[]) { entries.set(tabId, documents); for (const listener of listeners) listener(); },
  clear() { entries.clear(); for (const listener of listeners) listener(); },
  get(tabId: string) { return entries.get(tabId) ?? empty; },
  subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
};
export function useChatDocuments(tabId: string) {
  return useSyncExternalStore(chatDocumentsStore.subscribe, () => chatDocumentsStore.get(tabId), () => empty);
}
