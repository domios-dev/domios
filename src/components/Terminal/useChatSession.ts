import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import {
  matchedOptimisticEchoes,
  type NormalizedChatMessage as ChatMessage,
  type OptimisticEcho,
} from "@codeterm/chat-core";
import { detectChatPrefix, splitChatPrefixedMessages, type ChatOrigin } from "../../lib/chatPrefixes";

const INITIAL_VISIBLE_MESSAGES = 220;
const VISIBLE_MESSAGE_STEP = 160;

interface ChatStartResult {
  text: string;
  offset: number;
  start_offset?: number;
  pending?: boolean;
  messages?: ChatMessage[];
  cursor?: string;
}

interface ChatMsgEvent {
  pane_id: string;
  messages: ChatMessage[];
  cursor?: string;
}

function mergeById(prev: ChatMessage[], incoming: ChatMessage[]): ChatMessage[] {
  const map = new Map<string, ChatMessage>();
  for (const m of prev) map.set(m.id, m);
  for (const m of incoming) map.set(m.id, m);
  return Array.from(map.values()).sort(
    (a, b) => Number(a.timestamp ?? 0) - Number(b.timestamp ?? 0),
  );
}

function toEchoes(msgs: ChatMessage[]): OptimisticEcho[] {
  return msgs.map((m) => ({ id: m.id, content: m.content, sentAtMs: Number(m.timestamp) || 0 }));
}

function groupToolResults(messages: ChatMessage[]): ChatMessage[] {
  const grouped: ChatMessage[] = [];
  const toolIndex = new Map<string, number>();
  for (const message of messages) {
    if (message.type === "tool_call" && message.toolId) {
      toolIndex.set(message.toolId, grouped.length);
      grouped.push(message);
      continue;
    }
    if (message.type === "tool_result" && message.toolId) {
      const index = toolIndex.get(message.toolId);
      if (index != null) {
        const existing = grouped[index];
        const toolResult = existing.toolResult
          ? `${existing.toolResult}\n${message.content}`
          : message.content;
        grouped[index] = { ...existing, toolResult };
        continue;
      }
    }
    grouped.push(message);
  }
  return grouped;
}

export function useChatSession({
  paneId,
  agentSessionId,
  onError,
}: {
  paneId: string;
  agentSessionId: string | null;
  onError?: (msg: string) => void;
}) {
  const [structured, setStructured] = useState<ChatMessage[] | null>(null);
  const [visibleMessageCount, setVisibleMessageCount] = useState(INITIAL_VISIBLE_MESSAGES);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [optimistic, setOptimistic] = useState<ChatMessage[]>([]);
  const onErrorRef = useRef(onError);

  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    (async () => {
      try {
        const res = await invoke<ChatStartResult>("chat_start", { paneId });
        if (cancelled) return;
        setError(null);
        setVisibleMessageCount(INITIAL_VISIBLE_MESSAGES);
        setLoading(false);
        if (res.pending) return;
        setStructured(res.messages ?? []);
        unlisten = await listen<ChatMsgEvent>(
          `pane-chat-msg-${paneId}`,
          (ev) => {
            if (cancelled) return;
            const incoming = ev.payload.messages;
            if (!incoming?.length) return;
            setStructured((prev) => mergeById(prev ?? [], incoming));
          },
        );
      } catch (e) {
        if (cancelled) return;
        const msg = e instanceof Error ? e.message : String(e);
        setError(msg);
        setLoading(false);
        onErrorRef.current?.(msg);
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
      void invoke("chat_stop", { paneId }).catch(() => {});
    };
  }, [paneId, agentSessionId]);

  useEffect(() => {
    if (error?.includes("chat_send_raw")) setError(null);
  }, [error]);

  const parsed = useMemo<ChatMessage[]>(() => structured ?? [], [structured]);

  const allMessages = useMemo(() => {
    const grouped = groupToolResults(parsed);
    if (optimistic.length === 0) return grouped;
    // Render every real user turn (mergeById already dedupes by stable id — no
    // content-collapse that would swallow a legitimately repeated message).
    // A bubble is hidden only once a real echo positively claims it.
    const parsedUsers = parsed.filter((m) => m.type === "user");
    const { matchedIds } = matchedOptimisticEchoes(toEchoes(optimistic), parsedUsers);
    const matched = new Set(matchedIds);
    const stillPending = optimistic.filter((m) => !matched.has(m.id));
    return [...grouped, ...stillPending];
  }, [parsed, optimistic]);

  const enrichedMessages = useMemo(() => {
    const visible = allMessages.slice(-visibleMessageCount);
    const out: Array<{ msg: ChatMessage; origin: ChatOrigin | undefined }> = [];
    for (const m of visible) {
      if (m.type !== "user") {
        out.push({ msg: m, origin: undefined });
        continue;
      }
      const splitPrefixes = splitChatPrefixedMessages(m.content);
      if (splitPrefixes && splitPrefixes.length > 1) {
        splitPrefixes.forEach((prefix, index) => {
          out.push({
            msg: { ...m, id: `${m.id}-prefix-${index}`, content: prefix.body },
            origin: prefix.origin,
          });
        });
        continue;
      }
      const prefix = detectChatPrefix(m.content);
      if (!prefix) {
        out.push({ msg: m, origin: undefined });
        continue;
      }
      out.push({
        msg: { ...m, content: prefix.body },
        origin: prefix.origin,
      });
    }
    return out;
  }, [allMessages, visibleMessageCount]);

  const hiddenMessageCount = Math.max(0, allMessages.length - enrichedMessages.length);

  const loadOlder = useCallback(() => {
    if (hiddenMessageCount > 0) {
      setVisibleMessageCount((count) => count + VISIBLE_MESSAGE_STEP);
    }
  }, [hiddenMessageCount]);

  const addOptimistic = useCallback((content: string) => {
    const id = `opt-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
    setOptimistic((prev) => [
      ...prev,
      {
        id,
        type: "user",
        content,
        timestamp: String(Date.now()),
      },
    ]);
    return id;
  }, []);

  /** Replace an optimistic bubble's content in place — used to morph a
   * "transcribing…" placeholder into the final transcript (or an error)
   * without the bubble shifting or re-mounting. */
  const updateOptimistic = useCallback((id: string, content: string) => {
    setOptimistic((prev) =>
      prev.map((m) => (m.id === id ? { ...m, content, timestamp: String(Date.now()) } : m)),
    );
  }, []);

  // Garbage-collect bubbles whose real echo has landed. NO timer eviction:
  // a bubble lives until a parsed user message positively, consume-once claims
  // it. If the echo never arrives the bubble stays — visible, not silently lost.
  useEffect(() => {
    if (optimistic.length === 0) return;
    const parsedUsers = parsed.filter((m) => m.type === "user");
    const { matchedIds } = matchedOptimisticEchoes(toEchoes(optimistic), parsedUsers);
    if (matchedIds.length === 0) return;
    const matched = new Set(matchedIds);
    setOptimistic((prev) => prev.filter((m) => !matched.has(m.id)));
  }, [parsed, optimistic]);

  return {
    loading,
    error,
    setError,
    allMessages,
    enrichedMessages,
    hiddenMessageCount,
    loadOlder,
    addOptimistic,
    updateOptimistic,
  };
}
