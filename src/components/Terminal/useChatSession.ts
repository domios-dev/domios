import {
  type MutableRefObject,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import type {
  NormalizedChatMessage as ChatMessage,
  TranscriptBatch,
  TranscriptOutcome,
  TranscriptRange,
} from "@codeterm/chat-core";
import { onResync } from "../../lib/resync";
import { CHAT_REATTACHED_EVENT } from "./chatReattach";
import {
  canAdoptTranscriptAuthority,
  type ChatAttachmentState,
  type ChatBackfillResult,
  type ChatMsgEvent,
  type ChatStartResult,
} from "./chatSessionAuthority";
import { useChatAttachment } from "./chatAttachment";
import { listenForTestChatReplies } from "./chatTestBridge";
import {
  tailSignatureOf,
  type ChatSessionDiagnosticEvent,
  type ChatSessionDiagnosticSink,
} from "./chatSessionDiagnostics";
import {
  commitTranscriptBatch,
  transcriptWindowKey,
  useTranscriptWindow,
} from "./transcriptStore";
import { useChatWindowView } from "./useChatWindowView";

export {
  TEST_CHAT_REPLY_EVENT,
  installCodeTermTestChatHandle,
  isCodeTermFrontendTestMode,
  listenForTestChatReplies,
  resetTestChatClockForCodeTerm,
  testChatReplyPayload,
  type ChatEventCommit,
  type TestChatHandle,
  type TestChatReplyDetail,
} from "./chatTestBridge";
export {
  paintTailSignature,
  tailSignatureOf,
  type ChatSessionDiagnosticEvent,
  type ChatSessionDiagnosticSink,
} from "./chatSessionDiagnostics";

/** Recovery is decided by reducer outcomes and observed authority, never by a
 *  deadline: these are the outcomes that require a fresh authoritative parse. */
function needsRecovery(outcome: TranscriptOutcome): boolean {
  return outcome === "recover_gap" || outcome === "recover_overlap";
}

function bindingKey(sessionId: string | null, nonce: string | null | undefined): string {
  return `${sessionId ?? ""}|${nonce ?? ""}`;
}

function batchRange(start: number | undefined, end: number | undefined): TranscriptRange | null {
  if (start == null || end == null || end <= 0) return null;
  return { start, end };
}

export function useChatSession({
  tabId,
  agentSessionId,
  sessionBindingNonce,
  onError,
  active = true,
  diagnosticSink,
  allowUnboundAuthority = false,
  followTailRef: suppliedFollowTailRef,
}: {
  tabId: string;
  agentSessionId: string | null;
  sessionBindingNonce: string | null | undefined;
  onError?: (msg: string) => void;
  /** Hidden chats must not run the daemon parser: false stops the session, true restarts it. */
  active?: boolean;
  diagnosticSink?: ChatSessionDiagnosticSink;
  /** Plugin chatBackend tabs have tab-owned backing without a PTY session id. */
  allowUnboundAuthority?: boolean;
  followTailRef?: MutableRefObject<boolean>;
}) {
  const windowKey = transcriptWindowKey(tabId, agentSessionId);
  const { transcript, anchorKey } = useTranscriptWindow(windowKey);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [listenerReady, setListenerReady] = useState(false);
  const [attachmentState, setAttachmentState] = useState<ChatAttachmentState>("idle");
  const onErrorRef = useRef(onError);
  const diagnosticSinkRef = useRef(diagnosticSink);
  const activeRef = useRef(active);
  const agentSessionIdRef = useRef(agentSessionId);
  const sessionBindingNonceRef = useRef(sessionBindingNonce);
  const allowUnboundAuthorityRef = useRef(allowUnboundAuthority);
  const attachmentStateRef = useRef(attachmentState);
  const windowKeyRef = useRef(windowKey);
  const previousAuthorityRef = useRef({
    sessionId: agentSessionId,
    sessionBindingNonce: sessionBindingNonce ?? null,
  });
  const adoptedAuthorityRef = useRef<{
    sourceSessionId: string | null;
    sessionBindingNonce: string | null;
  }>({
    sourceSessionId: agentSessionId,
    sessionBindingNonce: sessionBindingNonce ?? null,
  });
  /** Bindings this hook has actually subscribed to. An event carrying one of
   *  them after the tab moved on is proof our own subscription went stale. */
  const adoptedBindingsRef = useRef(new Set<string>());
  /** A chat that never held a session is not recovering history, it is new. */
  const hadBoundSessionRef = useRef(agentSessionId != null);
  const internalFollowTailRef = useRef(true);
  const followTailRef = suppliedFollowTailRef ?? internalFollowTailRef;
  const rotationQueuedRef = useRef(false);
  const recoveryQueuedRef = useRef(false);

  onErrorRef.current = onError;
  diagnosticSinkRef.current = diagnosticSink;
  activeRef.current = active;
  agentSessionIdRef.current = agentSessionId;
  sessionBindingNonceRef.current = sessionBindingNonce;
  allowUnboundAuthorityRef.current = allowUnboundAuthority;
  attachmentStateRef.current = attachmentState;
  windowKeyRef.current = windowKey;
  if (agentSessionId != null) hadBoundSessionRef.current = true;

  const emitDiagnostic = useCallback((event: ChatSessionDiagnosticEvent) => {
    diagnosticSinkRef.current?.(event);
  }, []);

  const observeSubscription = useCallback((
    edge: "owned" | "released",
    subscriptionId: string,
  ) => {
    emitDiagnostic({
      edge: edge === "owned" ? "subscription_owned" : "subscription_released",
      tabId,
      subscriptionId,
    });
  }, [emitDiagnostic, tabId]);

  const attachment = useChatAttachment(tabId, observeSubscription);
  const { begin: beginAttach, generation } = attachment;
  const attachmentRef = useRef(attachment);
  attachmentRef.current = attachment;

  const commitBatch = useCallback((
    kind: TranscriptBatch["kind"],
    authority: { sourceSessionId: string | null; sessionBindingNonce: string | null },
    range: TranscriptRange | null,
    rows: ChatMessage[],
  ) => commitTranscriptBatch(windowKeyRef.current, { kind, authority, range, rows }), []);

  const requestAttach = useCallback(() => {
    setError(null);
    setAttachmentState("attaching");
    beginAttach();
  }, [beginAttach]);

  /** Coalesce recovery to one re-attach per tick; the trigger is always an
   *  observed outcome, never elapsed time. */
  const requestRecoveryAttach = useCallback(() => {
    if (recoveryQueuedRef.current) return;
    recoveryQueuedRef.current = true;
    queueMicrotask(() => {
      recoveryQueuedRef.current = false;
      if (activeRef.current) requestAttach();
    });
  }, [requestAttach]);

  const markAttached = useCallback(() => {
    setAttachmentState("attached");
    setLoading(false);
  }, []);

  const commitLiveEvent = useCallback((payload: ChatMsgEvent): TranscriptOutcome => {
    if (payload.tab_id !== tabId || payload.messages.length === 0) return "ignored_empty";
    return commitBatch(
      "delta",
      {
        sourceSessionId: payload.source_session_id,
        sessionBindingNonce: payload.session_binding_nonce,
      },
      batchRange(payload.start_offset, payload.offset),
      payload.messages,
    );
  }, [commitBatch, tabId]);

  const loadOlderPage = useCallback(async (beforeOffset: number) => {
    const generationAtRequest = attachmentRef.current.generation;
    try {
      const res = await invoke<ChatBackfillResult>("chat_backfill", { tabId, beforeOffset });
      if (
        !attachmentRef.current.isCurrent(generationAtRequest)
        || !canAdoptTranscriptAuthority(
          res,
          agentSessionIdRef.current,
          sessionBindingNonceRef.current ?? null,
          allowUnboundAuthorityRef.current,
        )
      ) {
        return false;
      }
      const page = res.messages ?? [];
      if (page.length === 0) return false;
      return commitBatch(
        "backfill",
        {
          sourceSessionId: res.source_session_id,
          sessionBindingNonce: res.session_binding_nonce,
        },
        { start: res.start_offset, end: res.end_offset },
        page,
      ) === "adopted";
    } catch (e) {
      onErrorRef.current?.(e instanceof Error ? e.message : String(e));
      return false;
    }
  }, [commitBatch, tabId]);

  const view = useChatWindowView({
    windowKey,
    transcript,
    anchorKey,
    followTailRef,
    loadOlderPage,
  });
  const { clearOptimistic, committedRows } = view;

  useEffect(() => {
    clearOptimistic();
  }, [clearOptimistic, tabId]);

  useEffect(() => {
    if (!active) {
      setListenerReady(false);
      setLoading(false);
      setAttachmentState("idle");
      return;
    }
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    let unlistenTest: (() => void) | null = null;
    setLoading(true);
    (async () => {
      try {
        unlisten = await listen<ChatMsgEvent>(
          `tab-chat-msg-${tabId}`,
          (ev) => {
            if (cancelled) return;
            const diagnosticBase = {
              tabId,
              sourceSessionId: ev.payload.source_session_id,
              sessionBindingNonce: ev.payload.session_binding_nonce,
              messageIds: ev.payload.messages.map((message) => message.id),
              tailSignature: tailSignatureOf(ev.payload.messages),
            };
            emitDiagnostic({ edge: "event_arrival", ...diagnosticBase });
            const addressedToThisTab = ev.payload.tab_id === tabId;
            if (
              !addressedToThisTab
              || !canAdoptTranscriptAuthority(
                ev.payload,
                agentSessionIdRef.current,
                sessionBindingNonceRef.current ?? null,
                allowUnboundAuthorityRef.current,
              )
            ) {
              emitDiagnostic({ edge: "authority_drop", ...diagnosticBase });
              // Another tab's delta says nothing about this tab's subscription,
              // even when it carries a binding this tab once adopted.
              if (!addressedToThisTab) return;
              const ownStream = adoptedBindingsRef.current.has(bindingKey(
                ev.payload.source_session_id,
                ev.payload.session_binding_nonce,
              ));
              if (ownStream && attachmentStateRef.current === "attached") requestRecoveryAttach();
              return;
            }
            emitDiagnostic({ edge: "authority_adopt", ...diagnosticBase });
            adoptedAuthorityRef.current = {
              sourceSessionId: ev.payload.source_session_id,
              sessionBindingNonce: ev.payload.session_binding_nonce,
            };
            if (needsRecovery(commitLiveEvent(ev.payload))) requestRecoveryAttach();
            emitDiagnostic({ edge: "state_enqueued", ...diagnosticBase });
            if (attachmentStateRef.current !== "attached") markAttached();
          },
        );
        if (cancelled) {
          unlisten();
          return;
        }
        emitDiagnostic({ edge: "listener_owned", tabId });
        unlistenTest = listenForTestChatReplies(tabId, commitLiveEvent);
        setListenerReady(true);
        setAttachmentState("idle");
      } catch (e) {
        if (cancelled) return;
        const msg = e instanceof Error ? e.message : String(e);
        setAttachmentState("failed");
        setError(msg);
        setLoading(false);
        onErrorRef.current?.(msg);
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
      if (unlisten) emitDiagnostic({ edge: "listener_released", tabId });
      unlistenTest?.();
      setListenerReady(false);
      attachmentRef.current.release();
    };
  }, [
    active,
    commitLiveEvent,
    emitDiagnostic,
    markAttached,
    requestRecoveryAttach,
    tabId,
  ]);

  useEffect(() => {
    if (!active || !listenerReady) return;
    const owner = attachmentRef.current;
    const generationAtRequest = generation;
    const requestedSessionId = agentSessionIdRef.current;
    const requestedSessionBindingNonce = sessionBindingNonceRef.current ?? null;
    const requestedSubscriptionId = owner.requestedSubscriptionId();
    let cancelled = false;
    (async () => {
      try {
        emitDiagnostic({
          edge: "snapshot_request",
          tabId,
          sourceSessionId: requestedSessionId,
          sessionBindingNonce: requestedSessionBindingNonce,
        });
        const res = await invoke<ChatStartResult>("chat_start", {
          tabId,
          agentSessionId: requestedSessionId,
          subscriptionId: requestedSubscriptionId,
        });
        if (cancelled || !owner.isCurrent(generationAtRequest)) {
          owner.discardForeign(res.subscription_id);
          return;
        }
        const tabAuthorityUnchanged =
          agentSessionIdRef.current === requestedSessionId
          && (sessionBindingNonceRef.current ?? null) === requestedSessionBindingNonce;
        const canAdoptSnapshot =
          tabAuthorityUnchanged
          && canAdoptTranscriptAuthority(
            res,
            requestedSessionId,
            requestedSessionBindingNonce,
            allowUnboundAuthorityRef.current,
          );
        if (!canAdoptSnapshot) {
          owner.discardForeign(res.subscription_id);
          emitDiagnostic({
            edge: "authority_drop",
            tabId,
            sourceSessionId: res.source_session_id,
            sessionBindingNonce: res.session_binding_nonce,
            messageIds: (res.messages ?? []).map((message) => message.id),
            tailSignature: tailSignatureOf(res.messages ?? []),
          });
          if (requestedSessionId == null && !allowUnboundAuthorityRef.current) {
            owner.adopt(null);
            setAttachmentState("awaiting_session");
            setLoading(false);
            setError(null);
          }
          return;
        }
        owner.adopt(res.subscription_id ?? null);
        adoptedBindingsRef.current.add(
          bindingKey(res.source_session_id, res.session_binding_nonce),
        );
        adoptedAuthorityRef.current = {
          sourceSessionId: res.source_session_id,
          sessionBindingNonce: res.session_binding_nonce,
        };
        emitDiagnostic({
          edge: "snapshot_arrival",
          tabId,
          sourceSessionId: res.source_session_id,
          sessionBindingNonce: res.session_binding_nonce,
          messageIds: (res.messages ?? []).map((message) => message.id),
          subscriptionId: res.subscription_id ?? null,
          tailSignature: tailSignatureOf(res.messages ?? []),
          cursor: res.cursor ?? null,
          offset: res.offset ?? null,
          startOffset: res.start_offset ?? null,
        });
        setError(null);
        setLoading(false);
        if (res.pending) {
          // The store has no transcript yet; the first live event completes the
          // attach. Nothing here waits on elapsed time.
          setAttachmentState("awaiting_store");
          return;
        }
        markAttached();
        commitBatch(
          "snapshot",
          {
            sourceSessionId: res.source_session_id,
            sessionBindingNonce: res.session_binding_nonce,
          },
          batchRange(res.start_offset, res.offset),
          res.messages ?? [],
        );
      } catch (e) {
        if (cancelled || !owner.isCurrent(generationAtRequest)) return;
        const msg = e instanceof Error ? e.message : String(e);
        setAttachmentState("failed");
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
    commitBatch,
    emitDiagnostic,
    generation,
    listenerReady,
    markAttached,
    tabId,
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
      const adopted = adoptedAuthorityRef.current;
      if (
        adopted.sourceSessionId !== agentSessionIdRef.current
        || adopted.sessionBindingNonce !== (sessionBindingNonceRef.current ?? null)
      ) {
        attachmentRef.current.adopt(null);
      }
      requestAttach();
    });
  }, [active, agentSessionId, listenerReady, requestAttach, sessionBindingNonce]);

  const retry = useCallback(() => requestAttach(), [requestAttach]);

  useEffect(() => {
    if (!active) return;
    return onResync(requestAttach);
  }, [active, requestAttach]);

  useEffect(() => {
    const handleReattached = (event: Event) => {
      const detail = (event as CustomEvent<{ tabId?: string }>).detail;
      if (detail?.tabId === tabId) retry();
    };
    window.addEventListener(CHAT_REATTACHED_EVENT, handleReattached);
    return () => window.removeEventListener(CHAT_REATTACHED_EVENT, handleReattached);
  }, [tabId, retry]);

  useEffect(() => {
    if (error?.includes("chat_send_raw")) setError(null);
  }, [error]);

  useLayoutEffect(() => {
    if (transcript.segments.length === 0) return;
    emitDiagnostic({
      edge: "state_commit",
      tabId,
      sourceSessionId: adoptedAuthorityRef.current.sourceSessionId,
      sessionBindingNonce: adoptedAuthorityRef.current.sessionBindingNonce,
      messageIds: committedRows.map((message) => message.id),
      tailSignature: tailSignatureOf(committedRows),
    });
  }, [committedRows, emitDiagnostic, tabId, transcript]);

  return {
    loading,
    /** Waiting on history we once had; a first attach is not a recovery. */
    recovering: hadBoundSessionRef.current
      && (attachmentState === "awaiting_store" || attachmentState === "awaiting_session"),
    error,
    attachmentState,
    retry,
    setError,
    allMessages: view.allMessages,
    enrichedMessages: view.enrichedMessages,
    hiddenMessageCount: view.hiddenMessageCount,
    canBackfillOlder: view.canBackfillOlder,
    backfillLoading: view.backfillLoading,
    loadOlder: view.loadOlder,
    showLatest: view.showLatest,
    setHistoryReading: view.setHistoryReading,
    addOptimistic: view.addOptimistic,
    updateOptimistic: view.updateOptimistic,
  };
}
