import { useEffect, useSyncExternalStore } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { compactionHistoryKey, contextCompactionStore } from "./contextCompactionStore";

export function useContextCompaction(tabId: string, sessionId: string | null, messages: readonly NormalizedChatMessage[]) {
  const key = compactionHistoryKey(tabId, sessionId ?? "");
  const history = useSyncExternalStore(contextCompactionStore.subscribe, () => contextCompactionStore.get(key));
  const failure = useSyncExternalStore(contextCompactionStore.subscribe, () => contextCompactionStore.error(key));
  useEffect(() => {
    if (sessionId) void contextCompactionStore.hydrate(key);
  }, [key, sessionId]);
  const tailId = [...messages].reverse().find((message) => message.timelineMarker?.kind !== "context_compaction")?.id ?? null;
  useEffect(() => {
    if (sessionId) void contextCompactionStore.anchor(key, tailId);
  }, [key, sessionId, tailId, history.compacting]);
  useEffect(() => {
    if (sessionId) void contextCompactionStore.reconcile(key, messages);
  }, [key, messages, sessionId, history]);
  return { history, failure: history.rows.length > 0 ? failure : undefined };
}
