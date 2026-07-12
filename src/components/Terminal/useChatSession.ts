import { startTransition, type Dispatch, type SetStateAction, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import {
  attachToolResults,
  lastSessionResetIndex,
  mergeById,
  mergeTimelineMessages,
  matchedOptimisticEchoes,
  trimAfterSessionReset,
  type NormalizedChatMessage as ChatMessage,
  type OptimisticEcho,
} from "@codeterm/chat-core";
import { detectChatPrefix, splitChatPrefixedMessages, type ChatOrigin } from "../../lib/chatPrefixes";

const INITIAL_VISIBLE_MESSAGES = 220;
const VISIBLE_MESSAGE_STEP = 160;
const CHAT_RECOVERY_RETRY_MS = 1_000;
export const TEST_CHAT_REPLY_EVENT = "codeterm:test-chat-reply";
const TEST_CHAT_BASE_TIME_MS = 1_700_000_000_000;

type FrontendTestEnv = {
  DEV?: boolean;
  VITE_CODETERM_TEST?: string;
};

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

export interface TestChatReplyDetail {
  paneId: string;
  content?: string;
  id?: string;
  timestamp?: string;
  type?: ChatMessage["type"];
  provider?: string;
  messages?: ChatMessage[];
}

export interface TestChatHandle {
  reply: (paneId: string, content: string, options?: Omit<TestChatReplyDetail, "paneId" | "content" | "messages">) => void;
  pushMessages: (paneId: string, messages: ChatMessage[]) => void;
  resetClock: () => void;
}

declare global {
  interface Window {
    __codetermTestChat?: TestChatHandle;
  }
}

let testChatSequence = 0;

function importEnv(): FrontendTestEnv | undefined {
  return (import.meta as { env?: FrontendTestEnv }).env;
}

function isLocalDevOrigin(): boolean {
  if (typeof window === "undefined") return false;
  const { hostname, port, protocol } = window.location;
  if (protocol !== "http:") return false;
  if (port === "7685") return false;
  return hostname === "127.0.0.1" || hostname === "localhost";
}

export function isCodeTermFrontendTestMode(env: FrontendTestEnv | undefined = importEnv()): boolean {
  const authoritativeDevRuntime = env?.DEV === true || isLocalDevOrigin();
  if (!authoritativeDevRuntime) return false;
  if (env?.VITE_CODETERM_TEST === "1") return true;
  if (typeof process !== "undefined" && process.env.VITE_CODETERM_TEST === "1") return true;
  // Runtime fallback: vite env does not survive detached dev starts (build
  // gateway), so harnesses request test mode via the page URL instead.
  if (typeof window !== "undefined" && window.location?.search) {
    return new URLSearchParams(window.location.search).has("e2e");
  }
  return false;
}

export function resetTestChatClockForCodeTerm(): void {
  testChatSequence = 0;
}

function nextTestChatTimestamp(): string {
  const value = TEST_CHAT_BASE_TIME_MS + testChatSequence;
  testChatSequence += 1;
  return String(value);
}

export function testChatReplyPayload(detail: TestChatReplyDetail): ChatMsgEvent {
  if (detail.messages) {
    return { pane_id: detail.paneId, messages: detail.messages };
  }
  return {
    pane_id: detail.paneId,
    messages: [
      {
        id: detail.id ?? `test-assistant-${detail.paneId}-${testChatSequence}`,
        type: detail.type ?? "assistant",
        content: detail.content ?? "",
        timestamp: detail.timestamp ?? nextTestChatTimestamp(),
        provider: detail.provider ?? "test",
      },
    ],
  };
}

export function installCodeTermTestChatHandle(testMode = isCodeTermFrontendTestMode()): void {
  if (!testMode || typeof window === "undefined" || window.__codetermTestChat) return;
  window.__codetermTestChat = {
    reply(paneId, content, options = {}) {
      window.dispatchEvent(
        new CustomEvent(TEST_CHAT_REPLY_EVENT, {
          detail: { ...options, paneId, content } satisfies TestChatReplyDetail,
        }),
      );
    },
    pushMessages(paneId, messages) {
      window.dispatchEvent(
        new CustomEvent(TEST_CHAT_REPLY_EVENT, {
          detail: { paneId, messages } satisfies TestChatReplyDetail,
        }),
      );
    },
    resetClock: resetTestChatClockForCodeTerm,
  };
}

function applyChatMsgEvent(
  payload: ChatMsgEvent,
  paneId: string,
  setStructured: Dispatch<SetStateAction<ChatMessage[] | null>>,
  nextOrder: () => number,
): void {
  if (payload.pane_id !== paneId) return;
  const incoming = withClientOrder(payload.messages, nextOrder);
  if (!incoming?.length) return;
  startTransition(() => {
    setStructured((prev) => mergeById(prev ?? [], incoming));
  });
}

function listenForTestChatReplies(
  paneId: string,
  setStructured: Dispatch<SetStateAction<ChatMessage[] | null>>,
  nextOrder: () => number,
): (() => void) | null {
  if (!isCodeTermFrontendTestMode() || typeof window === "undefined") return null;
  installCodeTermTestChatHandle();
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<TestChatReplyDetail>).detail;
    if (!detail || detail.paneId !== paneId) return;
    applyChatMsgEvent(testChatReplyPayload(detail), paneId, setStructured, nextOrder);
  };
  window.addEventListener(TEST_CHAT_REPLY_EVENT, handler);
  return () => window.removeEventListener(TEST_CHAT_REPLY_EVENT, handler);
}

function toEchoes(msgs: ChatMessage[]): OptimisticEcho[] {
  return msgs.map((m) => ({ id: m.id, content: m.content, sentAtMs: Number(m.timestamp) || 0 }));
}

function withClientOrder(messages: ChatMessage[], nextOrder: () => number): ChatMessage[] {
  return messages.map((message) => (
    message.clientOrder == null ? { ...message, clientOrder: nextOrder() } : message
  ));
}

const MS_EPOCH_FLOOR = 1e12;
function timestampMs(ts: string | undefined): number | null {
  if (!ts) return null;
  const numeric = Number(ts);
  if (Number.isFinite(numeric)) {
    if (numeric <= 0) return null;
    return numeric < MS_EPOCH_FLOOR ? numeric * 1000 : numeric;
  }
  const parsed = Date.parse(ts);
  return Number.isNaN(parsed) ? null : parsed;
}

export function useChatSession({
  paneId,
  agentSessionId,
  onError,
  active = true,
}: {
  paneId: string;
  agentSessionId: string | null;
  onError?: (msg: string) => void;
  /** Hidden chats must not run the daemon parser: false stops the session, true restarts it. */
  active?: boolean;
}) {
  const [structured, setStructured] = useState<ChatMessage[] | null>(null);
  const [visibleMessageCount, setVisibleMessageCount] = useState(INITIAL_VISIBLE_MESSAGES);
  const [loading, setLoading] = useState(true);
  const [recovering, setRecovering] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [restartToken, setRestartToken] = useState(0);
  const [optimistic, setOptimistic] = useState<ChatMessage[]>([]);
  const onErrorRef = useRef(onError);
  const nextClientOrderRef = useRef(0);
  const nextClientOrder = useCallback(() => {
    const value = nextClientOrderRef.current;
    nextClientOrderRef.current += 1;
    return value;
  }, []);

  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  useEffect(() => {
    if (!active) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    let unlistenTest: (() => void) | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    (async () => {
      try {
        setLoading(true);
        unlisten = await listen<ChatMsgEvent>(
          `pane-chat-msg-${paneId}`,
          (ev) => {
            if (cancelled) return;
            applyChatMsgEvent(ev.payload, paneId, setStructured, nextClientOrder);
          },
        );
        if (cancelled) {
          unlisten();
          return;
        }
        unlistenTest = listenForTestChatReplies(paneId, setStructured, nextClientOrder);
        const res = await invoke<ChatStartResult>("chat_start", { paneId });
        if (cancelled) return;
        setError(null);
        setVisibleMessageCount(INITIAL_VISIBLE_MESSAGES);
        nextClientOrderRef.current = 0;
        if (res.pending) {
          setRecovering(true);
          setLoading(false);
          retryTimer = setTimeout(() => setRestartToken((token) => token + 1), CHAT_RECOVERY_RETRY_MS);
          return;
        }
        setRecovering(false);
        setLoading(false);
        setStructured(withClientOrder(res.messages ?? [], nextClientOrder));
      } catch (e) {
        if (cancelled) return;
        const msg = e instanceof Error ? e.message : String(e);
        setRecovering(false);
        setError(msg);
        setLoading(false);
        onErrorRef.current?.(msg);
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
      unlistenTest?.();
      if (retryTimer) clearTimeout(retryTimer);
      // /new and /clear pills don't stick when the session boundary changes.
      setOptimistic([]);
      void invoke("chat_stop", { paneId }).catch(() => {});
    };
  }, [paneId, agentSessionId, nextClientOrder, active, restartToken]);

  const retry = useCallback(() => {
    setError(null);
    setRecovering(true);
    setRestartToken((token) => token + 1);
  }, []);

  useEffect(() => {
    if (error?.includes("chat_send_raw")) setError(null);
  }, [error]);

  const parsed = useMemo<ChatMessage[]>(() => structured ?? [], [structured]);
  const resetParsed = useMemo(() => trimAfterSessionReset(parsed), [parsed]);
  const resetCutoffMs = useMemo(() => {
    const index = lastSessionResetIndex(parsed);
    return index >= 0 ? timestampMs(parsed[index]?.timestamp) : null;
  }, [parsed]);
  const liveOptimistic = useMemo(() => {
    if (resetCutoffMs == null) return optimistic;
    return optimistic.filter((m) => {
      const ts = timestampMs(m.timestamp);
      return ts == null || ts >= resetCutoffMs;
    });
  }, [optimistic, resetCutoffMs]);

  const allMessages = useMemo(() => {
    const grouped = attachToolResults(resetParsed);
    if (liveOptimistic.length === 0) return grouped;
    // content-collapse that would swallow a legitimately repeated message).
    const parsedUsers = resetParsed.filter((m) => m.type === "user" || m.type === "local_command");
    const { matchedIds } = matchedOptimisticEchoes(toEchoes(liveOptimistic), parsedUsers);
    const matched = new Set(matchedIds);
    const stillPending = liveOptimistic.filter((m) => !matched.has(m.id));
    return mergeTimelineMessages(grouped, stillPending);
  }, [resetParsed, liveOptimistic]);

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
        clientOrder: nextClientOrder(),
      },
    ]);
    return id;
  }, [nextClientOrder]);

  /** Replace an optimistic bubble's content in place — used to morph a
   * "transcribing…" placeholder into the final transcript (or an error)
   * without the bubble shifting or re-mounting. */
  const updateOptimistic = useCallback((id: string, content: string) => {
    setOptimistic((prev) =>
      prev.map((m) => (m.id === id ? { ...m, content, timestamp: String(Date.now()) } : m)),
    );
  }, []);

  useEffect(() => {
    if (optimistic.length === 0) return;
    const parsedUsers = resetParsed.filter((m) => m.type === "user" || m.type === "local_command");
    const { matchedIds } = matchedOptimisticEchoes(toEchoes(optimistic), parsedUsers);
    const matched = new Set(matchedIds);
    setOptimistic((prev) => {
      const next = prev.filter((m) => {
        if (matched.has(m.id)) return false;
        if (resetCutoffMs == null) return true;
        const ts = timestampMs(m.timestamp);
        return ts == null || ts >= resetCutoffMs;
      });
      return next.length === prev.length ? prev : next;
    });
  }, [resetParsed, resetCutoffMs, optimistic]);

  return {
    loading,
    recovering,
    error,
    retry,
    setError,
    allMessages,
    enrichedMessages,
    hiddenMessageCount,
    loadOlder,
    addOptimistic,
    updateOptimistic,
  };
}
