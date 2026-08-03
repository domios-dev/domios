import {
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import {
  getCachedChatHistoryWindow,
  mergeRetainedChatLive,
  retainChatSnapshotTail,
  setCachedChatHistoryWindow,
  type RetainedChatHistory,
} from "./chatWindow";

export function useChatHistoryRetention({
  tabId,
  agentSessionId,
  suppliedFollowTailRef,
}: {
  tabId: string;
  agentSessionId: string | null;
  suppliedFollowTailRef?: MutableRefObject<boolean>;
}) {
  const historyCacheKey = `${tabId}:${agentSessionId ?? ""}`;
  const [initialCachedWindow] = useState(() => getCachedChatHistoryWindow(historyCacheKey));
  const [retainedHistory, setRetainedHistory] = useState<RetainedChatHistory | null>(
    initialCachedWindow?.history ?? null,
  );
  const [windowAnchorKey, setWindowAnchorKey] = useState<string | null>(
    initialCachedWindow?.anchorKey ?? null,
  );
  const internalFollowTailRef = useRef(true);
  const followTailRef = suppliedFollowTailRef ?? internalFollowTailRef;
  const retainedHistoryCacheKeyRef = useRef(historyCacheKey);

  const setRetainedStructured = useCallback<Dispatch<SetStateAction<ChatMessage[] | null>>>((action) => {
    setRetainedHistory((previous) => {
      const previousMessages = previous?.messages ?? null;
      const next = typeof action === "function" ? action(previousMessages) : action;
      if (next == null) return null;
      if (!previous) return retainChatSnapshotTail(next);
      return mergeRetainedChatLive(previous, next);
    });
  }, []);

  const updateWindowAnchor = useCallback((action: SetStateAction<string | null>) => {
    setWindowAnchorKey((previous) => (
      typeof action === "function" ? action(previous) : action
    ));
  }, []);

  useEffect(() => {
    if (retainedHistoryCacheKeyRef.current !== historyCacheKey) return;
    setCachedChatHistoryWindow(
      historyCacheKey,
      retainedHistory
        ? { history: retainedHistory, anchorKey: windowAnchorKey }
        : null,
    );
  }, [historyCacheKey, retainedHistory, windowAnchorKey]);

  useEffect(() => {
    if (retainedHistoryCacheKeyRef.current === historyCacheKey) return;
    retainedHistoryCacheKeyRef.current = historyCacheKey;
    const cached = getCachedChatHistoryWindow(historyCacheKey);
    followTailRef.current = cached?.anchorKey == null;
    setRetainedHistory(cached?.history ?? null);
    updateWindowAnchor(cached?.anchorKey ?? null);
  }, [followTailRef, historyCacheKey, updateWindowAnchor]);

  return {
    followTailRef,
    initialCachedWindow,
    retainedHistory,
    setRetainedHistory,
    setRetainedStructured,
    updateWindowAnchor,
    windowAnchorKey,
  };
}
