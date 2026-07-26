import { type Dispatch, type SetStateAction, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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
import { chatWindowRange, slideChatWindowOlder } from "./chatWindow";
import {
  canAdoptTranscriptAuthority,
  newChatSubscriptionId,
  useChatSubscription,
  type ChatAttachmentState,
} from "./chatSessionAuthority";

const BACKFILL_PAGE_MAX_BYTES = 256 * 1024;
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

interface ChatBackfillResult {
  text: string;
  start_offset: number;
  end_offset: number;
  messages?: ChatMessage[];
  source_session_id: string | null;
  session_binding_nonce: string | null;
}

interface ChatMsgEvent {
  pane_id: string;
  messages: ChatMessage[];
  cursor?: string;
  source_session_id: string | null;
  session_binding_nonce: string | null;
}

export type ChatSessionDiagnosticEvent =
  | {
      edge: "event_arrival" | "authority_adopt" | "authority_drop" | "state_enqueued";
      paneId: string;
      sourceSessionId: string | null;
      sessionBindingNonce: string | null;
      messageIds: string[];
      tailSignature: string | null;
    }
  | {
      edge: "snapshot_request";
      paneId: string;
      sourceSessionId: string | null;
      sessionBindingNonce: string | null;
    }
  | {
      edge: "snapshot_arrival";
      paneId: string;
      sourceSessionId: string | null;
      sessionBindingNonce: string | null;
      messageIds: string[];
      subscriptionId: string | null;
      /** Backend-sourced recovery coordinates so a re-attach after deactivation is
       *  causally attributable: the tailer's cursor/offset plus the transcript tail
       *  signature the snapshot recovered to. */
      tailSignature: string | null;
      cursor: string | null;
      offset: number | null;
      startOffset: number | null;
    }
  | {
      edge: "state_commit";
      paneId: string;
      sourceSessionId: string | null;
      sessionBindingNonce: string | null;
      messageIds: string[];
      tailSignature: string | null;
    }
  | {
      edge: "listener_owned" | "listener_released";
      paneId: string;
    }
  | {
      edge: "subscription_owned" | "subscription_released";
      paneId: string;
      subscriptionId: string;
    };

export type ChatSessionDiagnosticSink = (event: ChatSessionDiagnosticEvent) => void;

/** Tail signature of a message list for projection diagnostics: last id plus the
 *  last message's content length. Streaming grows the tail message under a stable
 *  id, so an id-only tail would make a committed-but-unpainted stream update
 *  invisible to the stall detector — the content length restores that progress. */
export function paintTailSignature(
  last: { id: string; content?: string | null } | undefined | null,
): string | null {
  return last ? `${last.id}#${last.content?.length ?? 0}` : null;
}

export function tailSignatureOf(
  messages: readonly { id: string; content?: string | null }[],
): string | null {
  return paintTailSignature(messages[messages.length - 1]);
}

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
  setStructured((prev) => {
    const existing = prev ?? [];
    return mergeById(existing, withPreservedClientOrder(payload.messages, existing, nextOrder, sourceSessionId));
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

/** Same-attach resync (rotation/retry/onResync): previous is live history, not
 *  eviction debris, so union it with the fresh snapshot. */
function mergeSnapshotWindow(
  previous: ChatMessage[],
  snapshot: ChatMessage[],
  nextOrder: () => number,
  sourceSessionId: string | null,
): ChatMessage[] {
  const freshSnapshot = withPreservedClientOrder(
    snapshot,
    previous,
    nextOrder,
    sourceSessionId,
  );
  return mergeById(previous, freshSnapshot);
}

/** Fresh listener attach (mount/reactivation): previous is a stale bounded
 *  window, so drop it and keep only deltas that raced in since this request. */
function replaceSnapshotWindow(
  previous: ChatMessage[],
  snapshot: ChatMessage[],
  requestOrder: number,
  nextOrder: () => number,
  sourceSessionId: string | null,
): ChatMessage[] {
  const liveSinceRequest = previous.filter(
    (message) => message.clientOrder != null && message.clientOrder >= requestOrder,
  );
  const freshSnapshot = withPreservedClientOrder(
    snapshot,
    previous,
    nextOrder,
    sourceSessionId,
  );
  return mergeById(freshSnapshot, liveSinceRequest);
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
  sessionBindingNonce,
  onError,
  active = true,
  diagnosticSink,
  allowUnboundAuthority = false,
}: {
  paneId: string;
  agentSessionId: string | null;
  sessionBindingNonce: string | null | undefined;
  onError?: (msg: string) => void;
  /** Hidden chats must not run the daemon parser: false stops the session, true restarts it. */
  active?: boolean;
  diagnosticSink?: ChatSessionDiagnosticSink;
  /** Plugin chatBackend panes have pane-owned backing without a PTY session id. */
  allowUnboundAuthority?: boolean;
}) {
  const [structured, setStructured] = useState<ChatMessage[] | null>(null);
  const [windowAnchorKey, setWindowAnchorKey] = useState<string | null>(null);
  const [backfillCursor, setBackfillCursor] = useState<number | null>(null);
  const [backfillLoading, setBackfillLoading] = useState(false);
  const [loading, setLoading] = useState(true);
  const [recovering, setRecovering] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [listenerReady, setListenerReady] = useState(false);
  const [attachGeneration, setAttachGeneration] = useState(0);
  const [attachmentState, setAttachmentState] = useState<ChatAttachmentState>("idle");
  const [optimistic, setOptimistic] = useState<ChatMessage[]>([]);
  const onErrorRef = useRef(onError);
  const diagnosticSinkRef = useRef(diagnosticSink);
  const nextClientOrderRef = useRef(0);
  const activeRef = useRef(active);
  const agentSessionIdRef = useRef(agentSessionId);
  const sessionBindingNonceRef = useRef(sessionBindingNonce);
  const allowUnboundAuthorityRef = useRef(allowUnboundAuthority);
  const attachmentSourceSessionIdRef = useRef(agentSessionId);
  const previousAuthorityRef = useRef({
    sessionId: agentSessionId,
    sessionBindingNonce: sessionBindingNonce ?? null,
  });
  const lifecycleRef = useRef(0);
  /** Lifecycle of the last merged snapshot: unchanged => resync (merge),
   *  new => fresh listener attach (replace). */
  const snapshotLifecycleRef = useRef<number | null>(null);
  const knownPaneIdRef = useRef<string | null>(null);
  const attachmentStateRef = useRef(attachmentState);
  const attachGenerationRef = useRef(attachGeneration);
  const pendingAuthorityRef = useRef<{
    sourceSessionId: string | null;
    sessionBindingNonce: string | null;
  }>({
    sourceSessionId: agentSessionId,
    sessionBindingNonce: sessionBindingNonce ?? null,
  });
  const requestedSubscriptionIdRef = useRef<string | null>(null);
  const rotationQueuedRef = useRef(false);
  const backfillInFlightRef = useRef(false);
  /** Backfilled pages get negative descending seq so equal-second boundary
   *  timestamps still sort before the snapshot (snapshot seq is 0-based). */
  const backfillSeqBaseRef = useRef(0);
  const pendingRevealRef = useRef(false);
  const nextClientOrder = useCallback(() => {
    const value = nextClientOrderRef.current;
    nextClientOrderRef.current += 1;
    return value;
  }, []);

  const emitDiagnostic = useCallback((event: ChatSessionDiagnosticEvent) => {
    diagnosticSinkRef.current?.(event);
  }, []);

  const observeSubscription = useCallback((
    edge: "owned" | "released",
    subscriptionId: string,
  ) => {
    emitDiagnostic({
      edge: edge === "owned" ? "subscription_owned" : "subscription_released",
      paneId,
      subscriptionId,
    });
  }, [emitDiagnostic, paneId]);

  const {
    release: releaseActiveSubscription,
    replace: replaceActiveSubscription,
    stopUnadopted: stopUnadoptedSubscription,
  } = useChatSubscription(paneId, observeSubscription);

  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  diagnosticSinkRef.current = diagnosticSink;
  activeRef.current = active;
  agentSessionIdRef.current = agentSessionId;
  sessionBindingNonceRef.current = sessionBindingNonce;
  allowUnboundAuthorityRef.current = allowUnboundAuthority;
  attachmentStateRef.current = attachmentState;
  attachGenerationRef.current = attachGeneration;

  useEffect(() => {
    setOptimistic([]);
  }, [paneId]);

  const requestAttach = useCallback(() => {
    const nextSessionId = agentSessionIdRef.current;
    attachmentSourceSessionIdRef.current = nextSessionId;
    requestedSubscriptionIdRef.current = newChatSubscriptionId(paneId);
    setError(null);
    setAttachmentState("attaching");
    setAttachGeneration((generation) => {
      const nextGeneration = generation + 1;
      attachGenerationRef.current = nextGeneration;
      return nextGeneration;
    });
  }, [paneId]);

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
    const requestedSubscriptionId = newChatSubscriptionId(paneId);
    requestedSubscriptionIdRef.current = requestedSubscriptionId;
    attachmentSourceSessionIdRef.current = agentSessionIdRef.current;
    let unlisten: UnlistenFn | null = null;
    let unlistenTest: (() => void) | null = null;
    if (knownPaneIdRef.current !== paneId) {
      knownPaneIdRef.current = paneId;
      nextClientOrderRef.current = 0;
      backfillSeqBaseRef.current = 0;
      setStructured(null);
      setWindowAnchorKey(null);
      setBackfillCursor(null);
      setLoading(true);
    }
    (async () => {
      try {
        unlisten = await listen<ChatMsgEvent>(
          `pane-chat-msg-${paneId}`,
          (ev) => {
            if (lifecycleRef.current !== lifecycle) return;
            const diagnosticBase = {
              paneId,
              sourceSessionId: ev.payload.source_session_id,
              sessionBindingNonce: ev.payload.session_binding_nonce,
              messageIds: ev.payload.messages.map((message) => message.id),
              tailSignature: tailSignatureOf(ev.payload.messages),
            };
            emitDiagnostic({ edge: "event_arrival", ...diagnosticBase });
            const paneSessionId = agentSessionIdRef.current;
            const paneSessionBindingNonce = sessionBindingNonceRef.current ?? null;
            if (
              ev.payload.pane_id !== paneId
              || !canAdoptTranscriptAuthority(
                ev.payload,
                paneSessionId,
                paneSessionBindingNonce,
                allowUnboundAuthorityRef.current,
              )
            ) {
              emitDiagnostic({ edge: "authority_drop", ...diagnosticBase });
              return;
            }
            emitDiagnostic({ edge: "authority_adopt", ...diagnosticBase });
            pendingAuthorityRef.current = {
              sourceSessionId: ev.payload.source_session_id,
              sessionBindingNonce: ev.payload.session_binding_nonce,
            };
            if (ev.payload.source_session_id != null) {
              attachmentSourceSessionIdRef.current = ev.payload.source_session_id;
            }
            applyChatMsgEvent(ev.payload, paneId, setStructured, nextClientOrder, attachmentSourceSessionIdRef.current);
            emitDiagnostic({ edge: "state_enqueued", ...diagnosticBase });
            if (attachmentStateRef.current !== "attached") markAttached();
          },
        );
        if (lifecycleRef.current !== lifecycle) {
          unlisten();
          return;
        }
        emitDiagnostic({ edge: "listener_owned", paneId });
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
      if (unlisten) emitDiagnostic({ edge: "listener_released", paneId });
      unlistenTest?.();
      setListenerReady(false);
      releaseActiveSubscription();
    };
  }, [
    active,
    emitDiagnostic,
    markAttached,
    nextClientOrder,
    paneId,
    releaseActiveSubscription,
  ]);

  useEffect(() => {
    if (!active || !listenerReady) return;
    const lifecycle = lifecycleRef.current;
    const generation = attachGeneration;
    const requestOrder = nextClientOrderRef.current;
    const requestedSessionId = agentSessionIdRef.current;
    const requestedSessionBindingNonce = sessionBindingNonceRef.current ?? null;
    const requestedSubscriptionId =
      requestedSubscriptionIdRef.current ?? newChatSubscriptionId(paneId);
    requestedSubscriptionIdRef.current = requestedSubscriptionId;
    let cancelled = false;
    (async () => {
      try {
        emitDiagnostic({
          edge: "snapshot_request",
          paneId,
          sourceSessionId: requestedSessionId,
          sessionBindingNonce: requestedSessionBindingNonce,
        });
        const res = await invoke<ChatStartResult>("chat_start", {
          paneId,
          agentSessionId: requestedSessionId,
          subscriptionId: requestedSubscriptionId,
        });
        const currentLifecycle = lifecycleRef.current === lifecycle;
        const currentGeneration = attachGenerationRef.current === generation;
        if (!currentLifecycle || cancelled || !currentGeneration) {
          stopUnadoptedSubscription(res.subscription_id);
          return;
        }
        const paneAuthorityUnchanged =
          agentSessionIdRef.current === requestedSessionId
          && (sessionBindingNonceRef.current ?? null) === requestedSessionBindingNonce;
        const canAdoptSnapshot =
          paneAuthorityUnchanged
          && canAdoptTranscriptAuthority(
            res,
            requestedSessionId,
            requestedSessionBindingNonce,
            allowUnboundAuthorityRef.current,
          );
        if (!canAdoptSnapshot) {
          stopUnadoptedSubscription(res.subscription_id);
          emitDiagnostic({
            edge: "authority_drop",
            paneId,
            sourceSessionId: res.source_session_id,
            sessionBindingNonce: res.session_binding_nonce,
            messageIds: (res.messages ?? []).map((message) => message.id),
            tailSignature: tailSignatureOf(res.messages ?? []),
          });
          if (requestedSessionId == null && !allowUnboundAuthorityRef.current) {
            replaceActiveSubscription(null);
            setAttachmentState("awaiting_session");
            setRecovering(true);
            setLoading(false);
          }
          return;
        }
        replaceActiveSubscription(res.subscription_id ?? null);
        pendingAuthorityRef.current = {
          sourceSessionId: res.source_session_id,
          sessionBindingNonce: res.session_binding_nonce,
        };
        emitDiagnostic({
          edge: "snapshot_arrival",
          paneId,
          sourceSessionId: res.source_session_id,
          sessionBindingNonce: res.session_binding_nonce,
          messageIds: (res.messages ?? []).map((message) => message.id),
          subscriptionId: res.subscription_id ?? null,
          tailSignature: tailSignatureOf(res.messages ?? []),
          cursor: res.cursor ?? null,
          offset: res.offset ?? null,
          startOffset: res.start_offset ?? null,
        });
        attachmentSourceSessionIdRef.current = res.source_session_id;
        setError(null);
        setLoading(false);
        if (res.pending) {
          setAttachmentState("awaiting_store");
          setRecovering(true);
          return;
        }
        markAttached();
        setBackfillCursor(res.start_offset != null && res.start_offset > 0 ? res.start_offset : null);
        const isFreshAttach = snapshotLifecycleRef.current !== lifecycle;
        snapshotLifecycleRef.current = lifecycle;
        setStructured((prev) =>
          isFreshAttach
            ? replaceSnapshotWindow(
                prev ?? [],
                res.messages ?? [],
                requestOrder,
                nextClientOrder,
                res.source_session_id,
              )
            : mergeSnapshotWindow(
                prev ?? [],
                res.messages ?? [],
                nextClientOrder,
                res.source_session_id,
              ),
        );
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
  }, [
    active,
    attachGeneration,
    emitDiagnostic,
    listenerReady,
    markAttached,
    nextClientOrder,
    paneId,
    replaceActiveSubscription,
    stopUnadoptedSubscription,
  ]);

  useEffect(() => {
    const previous = previousAuthorityRef.current;
    const next = {
      sessionId: agentSessionId,
      sessionBindingNonce: sessionBindingNonce ?? null,
    };
    previousAuthorityRef.current = next;
    if (
      (
        previous.sessionId === next.sessionId
        && previous.sessionBindingNonce === next.sessionBindingNonce
      )
      || !active
      || !listenerReady
    ) return;
    if (rotationQueuedRef.current) return;
    rotationQueuedRef.current = true;
    queueMicrotask(() => {
      rotationQueuedRef.current = false;
      if (!activeRef.current) return;
      const currentSessionId = agentSessionIdRef.current;
      const currentSessionBindingNonce = sessionBindingNonceRef.current ?? null;
      if (
        pendingAuthorityRef.current.sourceSessionId !== currentSessionId
        || pendingAuthorityRef.current.sessionBindingNonce !== currentSessionBindingNonce
      ) {
        replaceActiveSubscription(null);
      }
      requestAttach();
    });
  }, [
    active,
    agentSessionId,
    listenerReady,
    replaceActiveSubscription,
    requestAttach,
    sessionBindingNonce,
  ]);

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

  useLayoutEffect(() => {
    if (structured == null) return;
    emitDiagnostic({
      edge: "state_commit",
      paneId,
      sourceSessionId: pendingAuthorityRef.current.sourceSessionId,
      sessionBindingNonce: pendingAuthorityRef.current.sessionBindingNonce,
      messageIds: structured.map((message) => message.id),
      tailSignature: tailSignatureOf(structured),
    });
  }, [emitDiagnostic, paneId, structured]);

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

  const windowRange = useMemo(
    () => chatWindowRange(allTimelineItems, windowAnchorKey),
    [allTimelineItems, windowAnchorKey],
  );

  const enrichmentCacheRef = useRef<EnrichmentCache>(new WeakMap());
  const enrichedMessages = useMemo(() => {
    const visible = allTimelineItems.slice(windowRange.start, windowRange.end);
    return enrichTimelineItems(visible, enrichmentCacheRef.current);
  }, [allTimelineItems, windowRange]);

  const hiddenMessageCount = windowRange.start;
  const hiddenNewerCount = Math.max(0, allTimelineItems.length - windowRange.end);
  const canBackfillOlder = backfillCursor != null && backfillCursor > 0;

  const loadOlder = useCallback(async () => {
    if (hiddenMessageCount > 0) {
      setWindowAnchorKey((prev) => slideChatWindowOlder(allTimelineItems, prev));
      return;
    }
    const cursor = backfillCursor;
    if (cursor == null || cursor <= 0 || backfillInFlightRef.current) return;
    const lifecycle = lifecycleRef.current;
    const generation = attachGenerationRef.current;
    backfillInFlightRef.current = true;
    setBackfillLoading(true);
    try {
      const res = await invoke<ChatBackfillResult>("chat_backfill", {
        paneId,
        beforeOffset: cursor,
        maxBytes: BACKFILL_PAGE_MAX_BYTES,
      });
      if (
        lifecycleRef.current !== lifecycle
        || attachGenerationRef.current !== generation
        || !canAdoptTranscriptAuthority(
          {
            source_session_id: res.source_session_id,
            session_binding_nonce: res.session_binding_nonce,
          },
          agentSessionIdRef.current,
          sessionBindingNonceRef.current ?? null,
          allowUnboundAuthorityRef.current,
        )
      ) {
        return;
      }
      // Progress guard: a page that cannot move the cursor toward 0 ends paging.
      const nextCursor = res.start_offset > 0 && res.start_offset < cursor ? res.start_offset : null;
      setBackfillCursor(nextCursor);
      const parsed = res.messages ?? [];
      if (parsed.length === 0) return;
      const seqBase = backfillSeqBaseRef.current - parsed.length;
      backfillSeqBaseRef.current = seqBase;
      const stamped = parsed.map((m, index) => ({
        ...m,
        seq: seqBase + index,
        sourceSessionId: m.sourceSessionId ?? res.source_session_id ?? undefined,
      }));
      pendingRevealRef.current = true;
      // Backfill goes on the prev side: existing ids win the dedupe and
      // equal-timestamp boundary ties keep the older page first.
      setStructured((prev) => mergeById(stamped, prev ?? []));
    } catch (e) {
      onErrorRef.current?.(e instanceof Error ? e.message : String(e));
    } finally {
      backfillInFlightRef.current = false;
      setBackfillLoading(false);
    }
  }, [hiddenMessageCount, allTimelineItems, backfillCursor, paneId]);

  // Reveal a merged backfill page as soon as it lands in the timeline —
  // state-change driven, never timer driven.
  useLayoutEffect(() => {
    if (!pendingRevealRef.current) return;
    pendingRevealRef.current = false;
    setWindowAnchorKey((prev) => slideChatWindowOlder(allTimelineItems, prev));
  }, [allTimelineItems]);

  const showLatest = useCallback(() => {
    setWindowAnchorKey(null);
  }, []);

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
    hiddenNewerCount,
    canBackfillOlder,
    backfillLoading,
    loadOlder,
    showLatest,
    addOptimistic,
    updateOptimistic,
  };
}
