import { useSyncExternalStore } from "react";
import { sessionChanges, type SessionChanges, type NormalizedChatMessage } from "@codeterm/chat-core";

const empty = sessionChanges([]);
const entries = new Map<string, SessionChanges>();
const projections = new Map<string, ReturnType<typeof createChangesInventory>>();
const authorities = new Map<string, string>();
const listeners = new Set<() => void>();
export const chatChangesStore = {
  bind(tabId: string, authority: string) {
    if (authorities.get(tabId) === authority) return;
    authorities.set(tabId, authority);
    projections.delete(tabId);
    const previous = entries.get(tabId);
    entries.delete(tabId);
    if (previous && (previous.turns.length || previous.cumulative.length)) for (const listener of listeners) listener();
  },
  set(tabId: string, messages: readonly NormalizedChatMessage[]) {
    let project = projections.get(tabId);
    if (!project) { project = createChangesInventory(); projections.set(tabId, project); }
    const next = project(messages);
    if (next === (entries.get(tabId) ?? empty)) return;
    entries.set(tabId, next);
    for (const listener of listeners) listener();
  },
  get(tabId: string) { return entries.get(tabId) ?? empty; },
  clear() { entries.clear(); authorities.clear(); projections.clear(); for (const listener of listeners) listener(); },
  subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
};
export function useChatChanges(tabId: string) {
  return useSyncExternalStore(chatChangesStore.subscribe, () => chatChangesStore.get(tabId), () => empty);
}

function createChangesInventory() {
  let cache = new Map<string, { inputs: readonly unknown[]; message: NormalizedChatMessage }>();
  let previous: NormalizedChatMessage[] = [];
  let result = empty;
  return (messages: readonly NormalizedChatMessage[]) => {
    const relevant: NormalizedChatMessage[] = [];
    const nextCache = new Map<string, { inputs: readonly unknown[]; message: NormalizedChatMessage }>();
    const editCalls = new Set(messages.filter((message) => message.fileDiffs?.length || message.toolEdits?.length)
      .map((message) => JSON.stringify([message.sourceSessionId, message.toolId])));
    const seen = new Set<string>();
    for (const message of messages) {
      const identity = `${message.sourceSessionId ?? ""}:${message.id}`;
      if (seen.has(identity)) continue;
      seen.add(identity);
      if (message.type !== "user" && !(message.toolId && editCalls.has(JSON.stringify([message.sourceSessionId, message.toolId]))) && !message.fileDiffs?.length && !message.toolEdits?.length) continue;
      const inputs = [message.sourceMessageId, message.type, message.timestamp,
        message.toolId, message.fileDiffs, message.toolEdits, message.toolError,
        message.toolInterrupted, message.toolExitCode, message.toolResult !== undefined];
      const cached = cache.get(identity);
      const record = cached && inputs.every((value, index) => value === cached.inputs[index])
        ? cached : { inputs, message };
      nextCache.set(identity, record);
      relevant.push(record.message);
    }
    cache = nextCache;
    if (relevant.length === previous.length && relevant.every((value, index) => value === previous[index])) return result;
    previous = relevant;
    const next = sessionChanges(relevant);
    if (JSON.stringify(next) !== JSON.stringify(result)) result = next;
    return result;
  };
}
