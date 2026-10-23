import { canRetryChatStart, runChatStart } from "./chatStartFlight";
import {
  type MutableRefObject,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import { transcriptOffsetKind } from "@codeterm/chat-core";
import type {
  NormalizedChatMessage as ChatMessage,
  TranscriptBatch,
  TranscriptOutcome,
  TranscriptRange,
} from "@codeterm/chat-core";
import { onResync } from "../../lib/resync";
import { frontendBootDiagnostic } from "../../lib/frontendBootDiagnostic";
import { resyncRequiresTranscriptRecovery } from "../../lib/resyncModel";
import { runtimeHistorySettled, type RuntimeHistoryState } from "../../lib/agentRuntimeState";
import { CHAT_REATTACHED_EVENT } from "./chatReattach";
import {
  canAdoptTranscriptAuthority,
  chatStartBatchKind,
  knownWindowForAuthority,
  attachmentActivity,
  attachmentStateForSnapshot,
  needsTranscriptAttachment,
  type TranscriptAuthority,
  type ChatAttachmentState,
  type ChatBackfillResult,
  type ChatMsgEvent,
  type ChatStartResult,
  type ChatUnavailableReason,
} from "./chatSessionAuthority";
import { useChatAttachment } from "./chatAttachment";
import { createChatAttachFlight } from "./chatAttachFlight";
import { isByteResumeCursor, resumeChatCursor, type ChatResumeResult } from "./chatResume";
import { listenForTestChatReplies } from "./chatTestBridge";
import {
  tailSignatureOf,
  type ChatSessionDiagnosticEvent,
  type ChatSessionDiagnosticSink,
} from "./chatSessionDiagnostics";
import {
  commitTranscriptBatch,
  expandTranscriptWindowGap,
  forgetTranscriptWindowCursor,
  readTranscriptWindow,
  readTranscriptMemo,
  retainedTranscriptMemo,
  rememberTranscriptWindow,
  markTranscriptSnapshotReady,
  reconcileTranscriptWindow,
  resetTranscriptWindow,
  stampConfirmedTranscriptRow,
  transcriptWindowKey,
  useTranscriptWindow,
} from "./transcriptStore";
import { reconcileTrackedChatMessages, type ChatSendStatus } from "./chatSendStatus";
import { observeDeliveryStatus } from "./chatSendStore";
import { clearPersistedChatScrollState } from "./chatViewport";
import { adoptChatStartTiming, beginChatStartTiming, chatStartTimingEdge } from "./chatStartTiming";
import { useChatWindowView } from "./useChatWindowView";
import { useChatActivationReconcile } from "./useChatActivationReconcile";

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
  return outcome === "recover_gap";
}

function bindingKey(sessionId: string | null, nonce: string | null | undefined): string {
  return `${sessionId ?? ""}|${nonce ?? ""}`;
}

function batchRange(start: number | undefined, end: number | undefined, kind?: "bytes" | "items"): TranscriptRange | null {
  if (start == null || end == null || end <= 0) return null;
  return { start, end, unit: kind };
}

export function useChatSession({
  tabId,
  agentSessionId,
  sessionBindingNonce,
  onError,
  active = true,
  retainAttachment = false,
  diagnosticSink,
  allowUnboundAuthority = false,
  followTailRef: suppliedFollowTailRef,
  history,
  live = true,
}: {
  tabId: string;
  agentSessionId: string | null;
  sessionBindingNonce: string | null | undefined;
  onError?: (msg: string) => void;
  /** Visibility starts the first attachment. Retained views keep it until unmount. */
  active?: boolean;
  retainAttachment?: boolean;
  diagnosticSink?: ChatSessionDiagnosticSink;
  /** Plugin chatBackend tabs have tab-owned backing without a PTY session id. */
  allowUnboundAuthority?: boolean;
  followTailRef?: MutableRefObject<boolean>;
  /** Runtime history replay state; its completion re-reads the transcript snapshot. */
  history?: RuntimeHistoryState;
  /** Rows keep committing to the store; a held view renders them when live again. */
  live?: boolean;
}) {
  const activatedRef = useRef(false);
  if (active) activatedRef.current = true;
  const attachmentActive = active || (retainAttachment && activatedRef.current);
  const windowKey = transcriptWindowKey(tabId, agentSessionId);
  const { transcript, anchorKey, snapshotBinding } = useTranscriptWindow(windowKey, live);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [listenerReady, setListenerReady] = useState(false);
  const [listenerGeneration, setListenerGeneration] = useState(0);
  const [attachmentState, setAttachmentState] = useState<ChatAttachmentState>("idle");
  const [transcriptUnavailable, setTranscriptUnavailable] =
    useState<ChatUnavailableReason | null>(null);
  const onErrorRef = useRef(onError);
  const diagnosticSinkRef = useRef(diagnosticSink);
  const activeRef = useRef(attachmentActive);
  const agentSessionIdRef = useRef(agentSessionId);
  const sessionBindingNonceRef = useRef(sessionBindingNonce);
  const allowUnboundAuthorityRef = useRef(allowUnboundAuthority);
  const attachmentStateRef = useRef(attachmentState);
  const windowKeyRef = useRef(windowKey);
  const requestedAuthorityRef = useRef<TranscriptAuthority | null>(null);
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
  const recoveryQueuedRef = useRef(false);
  const resumeCursorRef = useRef<{ token: string; offset: number; authority: string } | null>(null);
  const resumeFlightRef = useRef<ReturnType<typeof createChatAttachFlight<void>> | null>(null);
  if (!resumeFlightRef.current) resumeFlightRef.current = createChatAttachFlight<void>();
  const resumeSequenceRef = useRef(0);
  const resumeLifetimeRef = useRef(0);
  const coldRecoveryRef = useRef(false);
  const pendingLiveEventsRef = useRef<ChatMsgEvent[]>([]);
  const snapshotOnNextLiveEventRef = useRef(false);
  const sendStatusInFlightRef = useRef(false);
  const sendStatusQueuedRef = useRef(false);

  onErrorRef.current = onError;
  diagnosticSinkRef.current = diagnosticSink;
  activeRef.current = attachmentActive;
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
  const failedStartRef = useRef<string | null>(null);
  const { begin: beginAttach, generation } = attachment;
  const attachmentRef = useRef(attachment);
  attachmentRef.current = attachment;

  const commitBatch = useCallback((
    kind: TranscriptBatch["kind"],
    authority: { sourceSessionId: string | null; sessionBindingNonce: string | null },
    range: TranscriptRange | null,
    rows: ChatMessage[],
  ) => commitTranscriptBatch(windowKeyRef.current, { kind, authority, range, rows }), []);

  const reconcileSendStatuses = useCallback(async () => {
    if (sendStatusInFlightRef.current) {
      sendStatusQueuedRef.current = true;
      return;
    }
    sendStatusInFlightRef.current = true;
    try {
      do {
        sendStatusQueuedRef.current = false;
        const statuses = await reconcileTrackedChatMessages(tabId);
        for (const status of statuses) {
          stampConfirmedTranscriptRow(windowKeyRef.current, status);
        }
      } while (sendStatusQueuedRef.current);
    } catch {
      return;
    } finally {
      sendStatusInFlightRef.current = false;
    }
  }, [tabId]);

  const requestAttach = useCallback((cause = "attachment") => {
    if (!canRetryChatStart(attachmentStateRef.current, cause)) return;
    const requested = requestedAuthorityRef.current;
    if (attachmentStateRef.current === "attaching"
      && requested?.source_session_id === agentSessionIdRef.current
      && requested.session_binding_nonce === (sessionBindingNonceRef.current ?? null)) return;
    frontendBootDiagnostic("chat-scroll", { chat: { scope: tabId, edge: "cold_attach", at: performance.now(),
      state: { cause, state: attachmentStateRef.current, session: agentSessionIdRef.current, nonce: sessionBindingNonceRef.current } } });
    pendingLiveEventsRef.current = [];
    snapshotOnNextLiveEventRef.current = false;
    const authority = readTranscriptWindow(windowKeyRef.current).transcript.authority;
    if (authority != null && (
      authority.sourceSessionId !== agentSessionIdRef.current
      || authority.sessionBindingNonce !== (sessionBindingNonceRef.current ?? null)
    )) {
      resetTranscriptWindow(windowKeyRef.current);
      clearPersistedChatScrollState(windowKeyRef.current);
    }
    setError(null);
    setTranscriptUnavailable(null);
    attachmentStateRef.current = "attaching";
    setAttachmentState("attaching");
    requestedAuthorityRef.current = {
      source_session_id: agentSessionIdRef.current,
      session_binding_nonce: sessionBindingNonceRef.current ?? null,
    };
    resumeLifetimeRef.current += 1;
    resumeCursorRef.current = null;
    resumeFlightRef.current?.cancelPending();
    beginAttach();
    void reconcileSendStatuses();
  }, [beginAttach, reconcileSendStatuses, tabId]);

  const markAttached = useCallback(() => {
    markTranscriptSnapshotReady(windowKeyRef.current, sessionBindingNonceRef.current);
    adoptChatStartTiming(tabId);
    attachmentStateRef.current = "attached";
    setAttachmentState("attached");
    setTranscriptUnavailable(null);
    setLoading(false);
  }, [tabId]);

  const rememberCursor = useCallback((payload: Pick<ChatMsgEvent, "cursor" | "offset" | "source_session_id" | "session_binding_nonce"> & { file_length?: number },
    outcome: TranscriptOutcome) => {
    if (!isByteResumeCursor(payload.cursor ?? null) || payload.offset == null
      || !["adopted", "rotated", "ignored_empty"].includes(outcome)) return;
    const authority = bindingKey(payload.source_session_id, payload.session_binding_nonce);
    const previous = resumeCursorRef.current;
    if (previous?.authority === authority && previous.offset > payload.offset) return;
    resumeCursorRef.current = { token: payload.cursor!, offset: payload.offset, authority };
    if (payload.source_session_id != null) {
      const previousWindow = readTranscriptMemo(windowKeyRef.current);
      rememberTranscriptWindow(windowKeyRef.current, {
        source_session_id: payload.source_session_id,
        session_binding_nonce: payload.session_binding_nonce,
        end_offset: payload.offset,
        file_length: Math.max(payload.file_length ?? previousWindow?.file_length ?? 0, payload.offset),
        cursor: payload.cursor!,
      });
    }
  }, []);

  const commitLiveEvent = useCallback((payload: ChatMsgEvent): TranscriptOutcome => {
    if (payload.tab_id !== tabId) return "ignored_empty";
    const batch: TranscriptBatch = {
      kind: "delta",
      authority: {
        sourceSessionId: payload.source_session_id,
        sessionBindingNonce: payload.session_binding_nonce,
      },
      range: batchRange(payload.start_offset, payload.offset, payload.offset_kind),
      rows: payload.messages,
    };
    if (!snapshotOnNextLiveEventRef.current) {
      const outcome = commitTranscriptBatch(windowKeyRef.current, batch);
      rememberCursor(payload, outcome);
      return outcome;
    }
    snapshotOnNextLiveEventRef.current = false;
    const outcome = reconcileTranscriptWindow(
      windowKeyRef.current,
      { ...batch, kind: "snapshot" },
    );
    rememberCursor(payload, outcome);
    return outcome;
  }, [commitBatch, rememberCursor, tabId]);

  const requestRecoveryAttach = useCallback((cold = false, cause = "retry") => {
    coldRecoveryRef.current ||= cold;
    if (recoveryQueuedRef.current) return;
    recoveryQueuedRef.current = true;
    queueMicrotask(() => {
      recoveryQueuedRef.current = false;
      if (!activeRef.current) return;
      const cursor = resumeCursorRef.current;
      const authority = bindingKey(agentSessionIdRef.current, sessionBindingNonceRef.current);
      const forceCold = coldRecoveryRef.current;
      coldRecoveryRef.current = false;
      frontendBootDiagnostic("chat-scroll", { chat: { scope: tabId, edge: "recovery_requested", at: performance.now(),
        state: { cause, forceCold, state: attachmentStateRef.current, cursor: cursor?.token ?? null, authority } } });
      if (forceCold || attachmentStateRef.current !== "attached" || cursor == null || cursor.authority !== authority) {
        resumeCursorRef.current = null;
        if (forceCold) forgetTranscriptWindowCursor(windowKeyRef.current);
        requestAttach(cause);
        return;
      }
      const lifetime = resumeLifetimeRef.current;
      const isCurrent = () => activeRef.current && resumeLifetimeRef.current === lifetime
        && bindingKey(agentSessionIdRef.current, sessionBindingNonceRef.current) === authority;
      const sequence = ++resumeSequenceRef.current;
      void resumeFlightRef.current!.run(`${lifetime}:${sequence}`, async () => {
        const currentCursor = resumeCursorRef.current;
        if (!isCurrent() || currentCursor?.authority !== authority) return;
        const outcome = await resumeChatCursor(currentCursor.token,
          async token => {
            const result = await invoke<ChatResumeResult>("chat_resume", { tabId, cursor: token });
            frontendBootDiagnostic("chat-scroll", { chat: { scope: tabId, edge: "resume_result", at: performance.now(),
              state: { kind: result.kind, ...("reason" in result ? { reason: result.reason } : {}),
                ...("source_kind" in result ? { sourceKind: result.source_kind } : {}) } } });
            return result;
          },
          delta => {
            if (!canAdoptTranscriptAuthority(delta, agentSessionIdRef.current, sessionBindingNonceRef.current ?? null)) {
              throw new Error("Chat resume authority mismatch");
            }
            return !needsRecovery(commitLiveEvent({ ...delta, tab_id: tabId, offset_kind: "bytes" }));
          },
          isCurrent,
        );
        if (outcome === "attach" && isCurrent()) {
          resumeCursorRef.current = null;
          requestAttach("resume_fallback");
        }
      }).catch((error: unknown) => {
        if (isCurrent()) setError(error instanceof Error ? error.message : String(error));
      });
    });
  }, [commitLiveEvent, requestAttach, tabId]);

  const observeChatSendStatus = useCallback((status: ChatSendStatus) => {
    observeDeliveryStatus(status);
    stampConfirmedTranscriptRow(windowKeyRef.current, status);
    void reconcileSendStatuses();
  }, [reconcileSendStatuses, tabId]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    void listen<ChatSendStatus>("chat-send-status", ({ payload }) => {
      if (!cancelled && payload.tab_id === tabId) observeChatSendStatus(payload);
    }).then((release) => {
      if (cancelled) release();
      else {
        unlisten = release;
        void reconcileSendStatuses();
      }
    }).catch((error: unknown) => {
      console.warn("[chat-send] status subscription failed", error);
    });
    return () => { cancelled = true; unlisten?.(); };
  }, [tabId, observeChatSendStatus, reconcileSendStatuses]);

  const loadOlderPage = useCallback(async (beforeOffset: number, maxBytes: number) => {
    if (agentSessionIdRef.current !== agentSessionId || sessionBindingNonceRef.current !== sessionBindingNonce) return null;
    const generationAtRequest = attachmentRef.current.generation;
    const requestedAt = performance.now();
    try {
      const offsetKind = transcriptOffsetKind(readTranscriptWindow(windowKeyRef.current).transcript);
      const res = await invoke<ChatBackfillResult>("chat_backfill", { tabId, beforeOffset, offsetKind, maxBytes });
      if (
        !attachmentRef.current.isCurrent(generationAtRequest)
        || !canAdoptTranscriptAuthority(
          res,
          agentSessionIdRef.current,
          sessionBindingNonceRef.current ?? null,
          allowUnboundAuthorityRef.current,
        )
      ) {
        return null;
      }
      if (res.start_offset < 0 || res.start_offset >= beforeOffset) {
        return { cursor: res.start_offset, consumedBytes: 0, rows: 0 };
      }
      const page = res.messages ?? [];
      const receivedAt = performance.now();
      const outcome = commitBatch(
        "backfill",
        {
          sourceSessionId: res.source_session_id,
          sessionBindingNonce: res.session_binding_nonce,
        },
        { start: res.start_offset, end: res.end_offset, unit: res.offset_kind },
        page,
      );
      frontendBootDiagnostic("chat-scroll", { chat: { scope: tabId, edge: "backfill_merged", at: performance.now(),
        state: { beforeOffset, rows: page.length, requestMs: receivedAt - requestedAt, mergeMs: performance.now() - receivedAt, outcome } } });
      return outcome === "adopted" ? {
        cursor: res.start_offset,
        consumedBytes: offsetKind === "items" ? 0 : beforeOffset - res.start_offset,
        rows: page.length,
      } : null;
    } catch (e) {
      onErrorRef.current?.(e instanceof Error ? e.message : String(e));
      return null;
    }
  }, [agentSessionId, commitBatch, sessionBindingNonce, tabId]);

  const loadHistoryGap = useCallback(async (message: ChatMessage) => {
    const marker = message.timelineMarker;
    if (marker?.kind !== "history_gap") return;
    const generationAtRequest = attachmentRef.current.generation;
    const res = await invoke<{ messages?: ChatMessage[] }>("chat_history_page", {
      tabId, source: marker.source, sessionId: marker.session_id, beforeOffset: marker.before_offset,
    });
    if (!attachmentRef.current.isCurrent(generationAtRequest)) return;
    expandTranscriptWindowGap(windowKeyRef.current, message.id, res.messages ?? []);
  }, [tabId]);

  const view = useChatWindowView({
    tabId,
    windowKey,
    transcript,
    attachedAuthority: snapshotBinding === `${windowKey}|${sessionBindingNonce ?? ""}`
      ? { sourceSessionId: agentSessionId, sessionBindingNonce: sessionBindingNonce ?? null } : null,
    anchorKey,
    followTailRef,
    loadOlderPage,
  });
  const { committedRows } = view;

  useEffect(() => {
    if (!attachmentActive) {
      pendingLiveEventsRef.current = [];
      adoptedBindingsRef.current.clear();
      setListenerReady(false);
      setLoading(false);
      setTranscriptUnavailable(null);
      attachmentStateRef.current = "idle";
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
              if (ownStream && attachmentStateRef.current === "attached") requestRecoveryAttach(false, "authority_drop");
              return;
            }
            emitDiagnostic({ edge: "authority_adopt", ...diagnosticBase });
            adoptedAuthorityRef.current = {
              sourceSessionId: ev.payload.source_session_id,
              sessionBindingNonce: ev.payload.session_binding_nonce,
            };
            if (attachmentStateRef.current === "attaching") {
              pendingLiveEventsRef.current.push(ev.payload);
              return;
            }
            if (needsRecovery(commitLiveEvent(ev.payload))) requestRecoveryAttach(false, "recover_gap");
            void reconcileSendStatuses();
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
        requestAttach("listener_owned");
        setListenerReady(true);
      } catch (e) {
        if (cancelled) return;
        const msg = e instanceof Error ? e.message : String(e);
        attachmentStateRef.current = "failed";
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
      resumeLifetimeRef.current += 1;
      resumeCursorRef.current = null;
      resumeFlightRef.current?.cancelPending();
      pendingLiveEventsRef.current = [];
      adoptedBindingsRef.current.clear();
      attachmentStateRef.current = "idle";
      requestedAuthorityRef.current = null;
    };
  }, [
    attachmentActive,
    listenerGeneration,
    commitLiveEvent,
    emitDiagnostic,
    markAttached,
    rememberCursor,
    requestRecoveryAttach,
    reconcileSendStatuses,
    requestAttach,
    tabId,
  ]);

  useEffect(() => {
    if (!needsTranscriptAttachment(
      requestedAuthorityRef.current, agentSessionId, sessionBindingNonce, attachmentActive, listenerReady,
    )) return;
    const adopted = adoptedAuthorityRef.current;
    if (adopted.sourceSessionId !== agentSessionId
      || adopted.sessionBindingNonce !== (sessionBindingNonce ?? null)) {
      attachmentRef.current.adopt(null);
    }
    requestAttach("authority_changed");
  }, [attachmentActive, agentSessionId, listenerReady, requestAttach, sessionBindingNonce]);

  useEffect(() => {
    if (!attachmentActive || !listenerReady) return;
    const owner = attachmentRef.current;
    const generationAtRequest = generation;
    const requestedSessionId = agentSessionIdRef.current;
    const requestedSessionBindingNonce = sessionBindingNonceRef.current ?? null;
    const requestedSubscriptionId = owner.requestedSubscriptionId();
    if (failedStartRef.current === requestedSubscriptionId) return;
    const controller = new AbortController();
    let cancelled = false;
    (async () => {
      try {
        if (!owner.isCurrent(generationAtRequest)) return;
        emitDiagnostic({
          edge: "snapshot_request",
          tabId,
          sourceSessionId: requestedSessionId,
          sessionBindingNonce: requestedSessionBindingNonce,
        });
        const timing = beginChatStartTiming(tabId);
        const res = await runChatStart(tabId, requestedSubscriptionId, signal => invoke<ChatStartResult>("chat_start", {
            tabId, agentSessionId: requestedSessionId, subscriptionId: requestedSubscriptionId,
            knownWindow: knownWindowForAuthority(retainedTranscriptMemo(windowKeyRef.current),
              requestedSessionId, requestedSessionBindingNonce),
          }).then(result => {
            if (signal.aborted) owner.discardForeign(result.subscription_id);
            return result;
          }), controller.signal);
        chatStartTimingEdge(timing, "response", res);
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
            attachmentStateRef.current = "awaiting_session";
            setAttachmentState("awaiting_session");
            setLoading(false);
            setError(null);
            return;
          }
          // The tab never moved, so nothing will re-ask on its behalf; report
          // the mismatch instead of waiting on an attach that cannot complete.
          if (tabAuthorityUnchanged) {
            setTranscriptUnavailable("authority_mismatch");
            setLoading(false);
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
        const snapshotState = attachmentStateForSnapshot(res);
        attachmentStateRef.current = snapshotState;
        setAttachmentState(snapshotState);
        if (snapshotState === "awaiting_store") {
          // The store has no transcript yet; the first live event completes the
          // attach. Nothing here waits on elapsed time.
          if (res.source_missing === true) setTranscriptUnavailable("no_transcript_source");
          snapshotOnNextLiveEventRef.current = true;
          const pending = pendingLiveEventsRef.current.splice(0);
          if (pending.length > 0) {
            for (const event of pending) commitLiveEvent(event);
            markAttached();
          }
          return;
        }
        snapshotOnNextLiveEventRef.current = false;
        const snapshotRange = batchRange(res.start_offset, res.offset, res.offset_kind);
        const batchKind = chatStartBatchKind(res.kind);
        let recover = false;
        if (batchKind != null && (snapshotRange != null || (res.messages?.length ?? 0) > 0
          || (res.archived_messages?.length ?? 0) > 0)) {
          const snapshotOutcome = reconcileTranscriptWindow(
          windowKeyRef.current,
          {
            kind: batchKind,
            authority: {
              sourceSessionId: res.source_session_id,
              sessionBindingNonce: res.session_binding_nonce,
            },
            range: snapshotRange,
            rows: res.messages ?? [],
          },
          );
          if (batchKind === "snapshot" && (res.archived_messages?.length ?? 0) > 0) {
            reconcileTranscriptWindow(windowKeyRef.current, {
              kind: "snapshot",
              authority: {
                sourceSessionId: res.source_session_id,
                sessionBindingNonce: res.session_binding_nonce,
              },
              range: null,
              placement: "oldest",
              rows: res.archived_messages!,
            });
          }
          rememberCursor(res, snapshotOutcome);
          recover = needsRecovery(snapshotOutcome);
        } else {
          rememberCursor(res, "ignored_empty");
        }
        const pending = pendingLiveEventsRef.current.splice(0);
        for (const event of pending) {
          if (needsRecovery(commitLiveEvent(event))) recover = true;
        }
        markAttached();
        if (recover) requestRecoveryAttach(true, "pending_recover_gap");
      } catch (e) {
        if (cancelled || !owner.isCurrent(generationAtRequest)) return;
        failedStartRef.current = requestedSubscriptionId;
        const msg = e instanceof Error ? e.message : String(e);
        attachmentStateRef.current = "failed";
        setAttachmentState("failed");
        setError(msg);
        setLoading(false);
        onErrorRef.current?.(msg);
      }
    })();
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [
    attachmentActive,
    commitBatch,
    commitLiveEvent,
    emitDiagnostic,
    generation,
    listenerReady,
    markAttached,
    rememberCursor,
    requestRecoveryAttach,
    tabId,
  ]);

  const retry = useCallback(() => {
    if (!listenerReady) setListenerGeneration(previous => previous + 1);
    else requestRecoveryAttach();
  }, [listenerReady, requestRecoveryAttach]);

  const reconcileActivation = useCallback((cause: string) => {
    if (attachmentStateRef.current === "failed" && !listenerReady) {
      setListenerGeneration(previous => previous + 1);
    } else if (["attached", "failed"].includes(attachmentStateRef.current)) requestRecoveryAttach(false, cause);
  }, [listenerReady, requestRecoveryAttach]);
  useChatActivationReconcile(active, reconcileActivation);

  const historyRef = useRef(history);
  useEffect(() => {
    const previous = historyRef.current;
    historyRef.current = history;
    if (attachmentActive && runtimeHistorySettled(previous, history)) requestRecoveryAttach(true, "history_settled");
  }, [attachmentActive, history, requestRecoveryAttach]);
  const historyLoading = history === "loading";

  useEffect(() => {
    if (!attachmentActive) return;
    return onResync(reason => {
      if (resyncRequiresTranscriptRecovery(reason, attachmentStateRef.current === "failed")) {
        requestRecoveryAttach(reason === "epoch_changed", `resync:${reason}`);
      }
    });
  }, [attachmentActive, requestRecoveryAttach]);

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
    ...attachmentActivity(attachmentState, loading, hadBoundSessionRef.current, agentSessionId != null),
    error,
    attachmentState,
    snapshotReady: snapshotBinding === `${windowKey}|${sessionBindingNonce ?? ""}`,
    /** Set = the wait is over and it failed; the view reports it rather than
     *  spinning. Cleared by a fresh attach or an observed delta, never by time. */
    transcriptUnavailable,
    retry,
    setError,
    allMessages: view.allMessages,
    enrichedMessages: view.enrichedMessages,
    hiddenMessageCount: view.hiddenMessageCount,
    newerMessageCount: view.newerMessageCount,
    canBackfillOlder: view.canBackfillOlder || historyLoading,
    backfillLoading: view.backfillLoading || historyLoading,
    backfillNotice: view.backfillNotice,
    loadOlder: view.loadOlder,
    loadHistoryGap,
    showLatest: view.showLatest,
    prepareMessage: view.prepareMessage,
    setHistoryReading: view.setHistoryReading,
    addOptimistic: view.addOptimistic,
    updateOptimistic: view.updateOptimistic,
    observeChatSendStatus,
  };
}
