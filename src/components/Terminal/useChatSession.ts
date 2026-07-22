import { startTransition, type Dispatch, type SetStateAction, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import {
  reduceToolEvents,
  stabilizeTimelineItems,
  lastSessionResetIndex,
  mergeById,
  mergeTimelineMessages,
  matchedOptimisticEchoes,
  collapseDuplicateUserMessages,
  trimAfterSessionReset,
  type NormalizedChatMessage as ChatMessage,
  type TimelineItem,
  type OptimisticEcho,
} from "@codeterm/chat-core";
import { enrichTimelineItems, type EnrichmentCache } from "./chatEnrichment";
import { onResync } from "../../lib/resync";
import { CHAT_REATTACHED_EVENT } from "./chatReattach";

const INITIAL_VISIBLE_MESSAGES = 220;
const VISIBLE_MESSAGE_STEP = 160;
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
  source_session_id: string | null;
  session_binding_nonce: string | null;
  subscription_id?: string | null;
}

interface ChatMsgEvent {
  pane_id: string;
  messages: ChatMessage[];
  cursor?: string;
  source_session_id: string | null;
  session_binding_nonce: string | null;
}

interface TranscriptAuthority {
  source_session_id: string | null;
}

type ChatAttachmentState = "idle" | "attaching" | "awaiting_store" | "attached" | "failed";

export interface TestChatReplyDetail {
  paneId: string;
  content?: string;
  id?: string;
  timestamp?: string;
  type?: ChatMessage["type"];
  provider?: string;
  messages?: ChatMessage[];
  sourceSessionId?: string | null;
  sessionBindingNonce?: string | null;
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
const testChatListenerCounts = new Map<string, number>();
const pendingTestChatReplies: TestChatReplyDetail[] = [];

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
    return {
      pane_id: detail.paneId,
      messages: detail.messages,
      source_session_id: detail.sourceSessionId ?? null,
      session_binding_nonce: detail.sessionBindingNonce ?? null,
    };
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
    source_session_id: detail.sourceSessionId ?? null,
    session_binding_nonce: detail.sessionBindingNonce ?? null,
  };
}

function canAdoptTranscriptAuthority(
  incoming: TranscriptAuthority,
  attachedSessionId: string | null,
  paneSessionId: string | null,
): boolean {
  const sourceSessionId = incoming.source_session_id;
  if (sourceSessionId == null) return attachedSessionId == null;
  return sourceSessionId === attachedSessionId
    || sourceSessionId === paneSessionId
    || (attachedSessionId == null && paneSessionId == null);
}

let subscriptionSequence = 0;

function newSubscriptionId(paneId: string): string {
  const randomId = globalThis.crypto?.randomUUID?.();
  if (randomId) return randomId;
  subscriptionSequence += 1;
  return `${paneId}-${subscriptionSequence}`;
}

export function installCodeTermTestChatHandle(testMode = isCodeTermFrontendTestMode()): void {
  if (!testMode || typeof window === "undefined" || window.__codetermTestChat) return;
  window.__codetermTestChat = {
    reply(paneId, content, options = {}) {
      dispatchOrQueueTestChatReply({ ...options, paneId, content });
    },
    pushMessages(paneId, messages) {
      dispatchOrQueueTestChatReply({ paneId, messages });
    },
    resetClock: resetTestChatClockForCodeTerm,
  };
}

function dispatchOrQueueTestChatReply(detail: TestChatReplyDetail): void {
  if (testChatListenerCounts.get(detail.paneId)) {
    dispatchTestChatReply(detail);
    return;
  }
  pendingTestChatReplies.push(detail);
  dispatchTestChatReply(detail);
}

function dispatchTestChatReply(detail: TestChatReplyDetail): void {
  window.dispatchEvent(
    new CustomEvent(TEST_CHAT_REPLY_EVENT, { detail }),
  );
}

function drainPendingTestChatReplies(
  paneId: string,
  setStructured: Dispatch<SetStateAction<ChatMessage[] | null>>,
  nextOrder: () => number,
): void {
  for (let i = 0; i < pendingTestChatReplies.length;) {
    const detail = pendingTestChatReplies[i];
    if (!detail || detail.paneId !== paneId) {
      i += 1;
      continue;
    }
    pendingTestChatReplies.splice(i, 1);
    applyChatMsgEvent(testChatReplyPayload(detail), paneId, setStructured, nextOrder);
  }
}

function applyChatMsgEvent(
  payload: ChatMsgEvent,
  paneId: string,
  setStructured: Dispatch<SetStateAction<ChatMessage[] | null>>,
  nextOrder: () => number,
  sourceSessionId: string | null = null,
): void {
  if (payload.pane_id !== paneId) return;
  if (!payload.messages.length) return;
  startTransition(() => {
    setStructured((prev) => {
      const existing = prev ?? [];
      return mergeById(existing, withPreservedClientOrder(payload.messages, existing, nextOrder, sourceSessionId));
    });
  });
}

export function listenForTestChatReplies(
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
  testChatListenerCounts.set(paneId, (testChatListenerCounts.get(paneId) ?? 0) + 1);
  window.addEventListener(TEST_CHAT_REPLY_EVENT, handler);
  drainPendingTestChatReplies(paneId, setStructured, nextOrder);
  return () => {
    const nextCount = (testChatListenerCounts.get(paneId) ?? 1) - 1;
    if (nextCount <= 0) testChatListenerCounts.delete(paneId);
    else testChatListenerCounts.set(paneId, nextCount);
    window.removeEventListener(TEST_CHAT_REPLY_EVENT, handler);
  };
}

function toEchoes(msgs: ChatMessage[]): OptimisticEcho[] {
  return msgs.map((m) => ({
    id: m.id,
    content: m.content,
    sentAtMs: Number(m.timestamp) || 0,
    clientOrder: m.clientOrder,
  }));
}

function withPreservedClientOrder(
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
  sessionBindingNonce: string | null | undefined;
  onError?: (msg: string) => void;
  /** Hidden chats must not run the daemon parser: false stops the session, true restarts it. */
  active?: boolean;
}) {
  const [structured, setStructured] = useState<ChatMessage[] | null>(null);
  const [visibleMessageCount, setVisibleMessageCount] = useState(INITIAL_VISIBLE_MESSAGES);
  const [loading, setLoading] = useState(true);
  const [recovering, setRecovering] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [listenerReady, setListenerReady] = useState(false);
  const [attachGeneration, setAttachGeneration] = useState(0);
  const [attachmentState, setAttachmentState] = useState<ChatAttachmentState>("idle");
  const [optimistic, setOptimistic] = useState<ChatMessage[]>([]);
  const onErrorRef = useRef(onError);
  const nextClientOrderRef = useRef(0);
  const activeRef = useRef(active);
  const agentSessionIdRef = useRef(agentSessionId);
  const attachmentSourceSessionIdRef = useRef(agentSessionId);
  const previousSessionIdRef = useRef(agentSessionId);
  const lifecycleRef = useRef(0);
  const knownPaneIdRef = useRef<string | null>(null);
  const attachmentStateRef = useRef(attachmentState);
  const attachGenerationRef = useRef(attachGeneration);
  const activeSubscriptionIdRef = useRef<string | null>(null);
  const requestedSubscriptionIdRef = useRef<string | null>(null);
  const rotationQueuedRef = useRef(false);
  const nextClientOrder = useCallback(() => {
    const value = nextClientOrderRef.current;
    nextClientOrderRef.current += 1;
    return value;
  }, []);

  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  activeRef.current = active;
  agentSessionIdRef.current = agentSessionId;
  attachmentStateRef.current = attachmentState;
  attachGenerationRef.current = attachGeneration;

  useEffect(() => {
    setOptimistic([]);
  }, [paneId]);

  const requestAttach = useCallback(() => {
    const nextSessionId = agentSessionIdRef.current;
    const lineageSessionId = attachmentSourceSessionIdRef.current;
    if (nextSessionId != null && lineageSessionId !== nextSessionId) {
      attachmentSourceSessionIdRef.current = nextSessionId;
    }
    setError(null);
    setAttachmentState("attaching");
    setAttachGeneration((generation) => {
      const nextGeneration = generation + 1;
      attachGenerationRef.current = nextGeneration;
      return nextGeneration;
    });
  }, []);

  const markAttached = useCallback(() => {
    setAttachmentState("attached");
    setRecovering(false);
    setLoading(false);
  }, []);

  useEffect(() => {
    if (!active) {
      setListenerReady(false);
      setLoading(false);
      setAttachmentState("idle");
      return;
    }
    const lifecycle = lifecycleRef.current + 1;
    lifecycleRef.current = lifecycle;
    const requestedSubscriptionId = newSubscriptionId(paneId);
    requestedSubscriptionIdRef.current = requestedSubscriptionId;
    activeSubscriptionIdRef.current = null;
    attachmentSourceSessionIdRef.current = agentSessionIdRef.current;
    let unlisten: UnlistenFn | null = null;
    let unlistenTest: (() => void) | null = null;
    if (knownPaneIdRef.current !== paneId) {
      knownPaneIdRef.current = paneId;
      nextClientOrderRef.current = 0;
      setStructured(null);
      setVisibleMessageCount(INITIAL_VISIBLE_MESSAGES);
      setLoading(true);
    }
    (async () => {
      try {
        unlisten = await listen<ChatMsgEvent>(
          `pane-chat-msg-${paneId}`,
          (ev) => {
            if (lifecycleRef.current !== lifecycle) return;
            const attachedSessionId = attachmentSourceSessionIdRef.current;
            const paneSessionId = agentSessionIdRef.current;
            if (!canAdoptTranscriptAuthority(ev.payload, attachedSessionId, paneSessionId)) return;
            if (ev.payload.source_session_id != null) {
              attachmentSourceSessionIdRef.current = ev.payload.source_session_id;
            }
            applyChatMsgEvent(ev.payload, paneId, setStructured, nextClientOrder, attachmentSourceSessionIdRef.current);
            if (attachmentStateRef.current !== "attached") markAttached();
          },
        );
        if (lifecycleRef.current !== lifecycle) {
          unlisten();
          return;
        }
        unlistenTest = listenForTestChatReplies(paneId, setStructured, nextClientOrder);
        setListenerReady(true);
        setAttachmentState("idle");
      } catch (e) {
        if (lifecycleRef.current !== lifecycle) return;
        const msg = e instanceof Error ? e.message : String(e);
        setAttachmentState("failed");
        setError(msg);
        setLoading(false);
        onErrorRef.current?.(msg);
      }
    })();
    return () => {
      lifecycleRef.current += 1;
      unlisten?.();
      unlistenTest?.();
      setListenerReady(false);
      const activeSubscriptionId = activeSubscriptionIdRef.current;
      if (activeSubscriptionId) {
        activeSubscriptionIdRef.current = null;
        void invoke("chat_stop", { paneId, subscriptionId: activeSubscriptionId }).catch(() => {});
      }
    };
  }, [active, markAttached, nextClientOrder, paneId]);

  useEffect(() => {
    if (!active || !listenerReady) return;
    const lifecycle = lifecycleRef.current;
    const generation = attachGeneration;
    let cancelled = false;
    (async () => {
      try {
        const res = await invoke<ChatStartResult>("chat_start", {
          paneId,
          agentSessionId: agentSessionIdRef.current,
          subscriptionId: requestedSubscriptionIdRef.current,
        });
        const currentLifecycle = lifecycleRef.current === lifecycle;
        if (!currentLifecycle) {
          if (res.subscription_id) {
            void invoke("chat_stop", { paneId, subscriptionId: res.subscription_id }).catch(() => {});
          }
          return;
        }
        if (cancelled || attachGenerationRef.current !== generation) return;
        activeSubscriptionIdRef.current = res.subscription_id ?? null;
        attachmentSourceSessionIdRef.current = res.source_session_id ?? agentSessionIdRef.current;
        setError(null);
        setLoading(false);
        if (res.pending) {
          if (agentSessionIdRef.current == null) {
            markAttached();
            return;
          }
          setAttachmentState("awaiting_store");
          setRecovering(true);
          return;
        }
        markAttached();
        setStructured((prev) => mergeById(
          prev ?? [],
          withPreservedClientOrder(res.messages ?? [], prev ?? [], nextClientOrder, res.source_session_id),
        ));
      } catch (e) {
        if (cancelled || lifecycleRef.current !== lifecycle || attachGenerationRef.current !== generation) return;
        const msg = e instanceof Error ? e.message : String(e);
        setAttachmentState("failed");
        setRecovering(false);
        setError(msg);
        setLoading(false);
        onErrorRef.current?.(msg);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [active, attachGeneration, listenerReady, markAttached, nextClientOrder, paneId]);

  useEffect(() => {
    const previous = previousSessionIdRef.current;
    previousSessionIdRef.current = agentSessionId;
    if (
      previous === agentSessionId
      || !active
      || !listenerReady
    ) return;
    if (rotationQueuedRef.current) return;
    rotationQueuedRef.current = true;
    queueMicrotask(() => {
      rotationQueuedRef.current = false;
      if (activeRef.current) requestAttach();
    });
  }, [active, agentSessionId, listenerReady, requestAttach]);

  const retry = useCallback(() => {
    setRecovering(true);
    requestAttach();
  }, [requestAttach]);

  useEffect(() => {
    if (!active) return;
    return onResync(requestAttach);
  }, [active, requestAttach]);

  useEffect(() => {
    const handleReattached = (event: Event) => {
      const detail = (event as CustomEvent<{ paneId?: string }>).detail;
      if (detail?.paneId === paneId) retry();
    };
    window.addEventListener(CHAT_REATTACHED_EVENT, handleReattached);
    return () => window.removeEventListener(CHAT_REATTACHED_EVENT, handleReattached);
  }, [paneId, retry]);

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
    if (liveOptimistic.length === 0) return collapseDuplicateUserMessages(resetParsed);
    // content-collapse that would swallow a legitimately repeated message).
    const parsedUsers = resetParsed.filter((m) => m.type === "user" || m.type === "local_command");
    const { matchedIds } = matchedOptimisticEchoes(toEchoes(liveOptimistic), parsedUsers);
    const matched = new Set(matchedIds);
    const stillPending = liveOptimistic.filter((m) => !matched.has(m.id));
    return collapseDuplicateUserMessages(mergeTimelineMessages(resetParsed, stillPending));
  }, [resetParsed, liveOptimistic]);

  // Both layers reuse prior objects for unchanged messages so MessageBubble
  // memo keeps skipping the ~220 untouched bubbles on every streamed chunk.
  const timelineRef = useRef<TimelineItem[] | undefined>(undefined);
  const allTimelineItems = useMemo(() => {
    const stabilized = stabilizeTimelineItems(timelineRef.current, reduceToolEvents(allMessages));
    timelineRef.current = stabilized;
    return stabilized;
  }, [allMessages]);

  const enrichmentCacheRef = useRef<EnrichmentCache>(new WeakMap());
  const enrichedMessages = useMemo(() => {
    const visible = allTimelineItems.length > visibleMessageCount
      ? allTimelineItems.slice(-visibleMessageCount)
      : allTimelineItems;
    return enrichTimelineItems(visible, enrichmentCacheRef.current);
  }, [allTimelineItems, visibleMessageCount]);

  const hiddenMessageCount = Math.max(0, allTimelineItems.length - enrichedMessages.length);

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
    attachmentState,
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
