import { Fragment, useCallback, useDeferredValue, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "../../lib/bridge";
import {
  ChatSessionIntentLine,
  isSessionIntentOnlyMessage,
  MessageBubble,
  OutboundReportCard,
  sessionIntentFromMessages,
} from "@codeterm/chat-ui";
import { extractPlanRefs } from "@codeterm/chat-ui";
import type { InteractiveAnswerHandler, OutboundReportDescriptor } from "@codeterm/chat-ui";
import { AgentIcon } from "../AgentIcon";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import {
  tabAgentTypesEqual,
  tabFieldsEqual,
  tabTitleMapEqual,
  planNavContextEqual,
  selectTab,
  selectTabAgentTypes,
  selectTabAsks,
  selectTabTitleMap,
  selectPlanNavContext,
  tabAsksEqual,
} from "../../store/multiplexer/selectors";
import { useTabProvider, useShellSessionModel } from "../../lib/useTabProvider";
import { formatModelBadge } from "../../lib/formatModelBadge";
import { usePlanIndexOptional } from "../Canvas/PlansProvider";
import { usePlanTargetNavigation } from "../Canvas/teamViewNavigation";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import type { ApiConfig, PendingPrompt } from "../../lib/types";
import { ChatComposer } from "./ChatComposer";
import { PendingPromptCard } from "./PendingPromptCard";
import {
  AutoConfirmedPromptRows,
  PermissionAutoConfirmToggle,
  autoConfirmationsForSession,
} from "./PermissionAutoConfirm";
import { AgentStateIndicator } from "./AgentStateIndicator";
import type { AgentState } from "./AgentStateIndicator";
import { useAttachments } from "./useAttachments";
import { useChatSession, isCodeTermFrontendTestMode, paintTailSignature } from "./useChatSession";
import { chatProjectionMonitor, scheduleAfterPaint } from "./chatProjectionDiagnostics";
import { ChatModelBubble } from "../Extensions/ChatModelBubble";
import { ChatPresetBubble } from "../Extensions/ChatPresetBubble";
import { WatcherReadOnlyBar } from "./WatcherReadOnlyBar";
import { ScrollToLatestButton } from "./ScrollToLatestButton";
import { ChatSetupState, chatSetupStageLabel, chatSetupSubject, chatSetupView } from "./ChatSetupState";
import { usePlanRefResolver } from "./usePlanRefResolver";
import { chatPlanRefView } from "./chatPlanRef";
import { useChatScroll } from "./useChatScroll";
import { resolveTabNavigationTarget } from "./tabNavigation";
import { useMeshPeerNameResolver } from "./useMeshPeerNameResolver";
import { useAgentReportProjection } from "./agentReportProjection";
import { chatBodyView, chatRailsVisible, placeChatRails } from "./chatBodyVisibility";
import { messageTimeMs, normalizeEpochMs } from "./outboundReportPlacement";
import { ChatAskCard } from "../Notifications/ChatAskCard";
import { useChatMessageReveal } from "./useChatMessageReveal";
import { useSessionBindingSurface } from "./useSessionBindingSurface";
import { SessionBindingRecoveryForm } from "./SessionBindingRecoveryForm";

interface Props {
  tabId: string;
  api?: ApiConfig;
  onError?: (msg: string) => void;
  onOpenTerminalFallback?: () => void;
  uiFontScale?: number;
  isVisible?: boolean;
  /** Tab is backed by a plugin chatBackend (no PTY). Sends route through the
   *  tab send seam, and PTY-only affordances (terminal fallback, screen poll)
   *  are suppressed. */
  chatBackend?: boolean;
}

const DISABLED_PLAN_API: ApiConfig = { enabled: false, host: "127.0.0.1", port: 0, token: null, cors: false };

function isMicrophonePermissionError(message: string): boolean {
  const normalized = message.toLowerCase();
  if (normalized.includes("needs https")) return false;
  return (
    normalized.includes("microphone") ||
    normalized.includes("mic api") ||
    normalized.includes("media device") ||
    normalized.includes("getusermedia") ||
    normalized.includes("notallowederror") ||
    normalized.includes("denied permission") ||
    normalized.includes("permission denied") ||
    normalized.includes("not allowed by the user agent")
  );
}

function chatMessageAccessibleName(type: string, content: string): string {
  const compact = content.replace(/\s+/g, " ").trim();
  const excerpt = compact.length > 80 ? `${compact.slice(0, 77)}…` : compact;
  return excerpt ? `${type} message: ${excerpt}` : `${type} message`;
}

function OriginProviderAvatar({ providerId }: { providerId: string | null | undefined }) {
  if (!providerId) return null;
  return (
    <span
      className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-full border"
      style={{
        background: "color-mix(in srgb, var(--ct-surface, #161b22) 86%, var(--ct-accent, #58a6ff) 14%)",
        borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 28%, transparent)",
        color: "var(--ct-fg)",
      }}
      aria-hidden
    >
      <AgentIcon providerId={providerId} className="text-[15px]" />
    </span>
  );
}

export function TabSessionChat({
  tabId,
  api,
  onError,
  onOpenTerminalFallback,
  uiFontScale,
  isVisible = true,
  chatBackend = false,
}: Props) {
  if (isCodeTermFrontendTestMode()) {
    const counts = (window.__renderCounts ??= {});
    counts[tabId] = (counts[tabId] ?? 0) + 1;
    window.__chatProjection ??= chatProjectionMonitor;
  }

  const touchMode = useTouchOptimizedUi();
  const lastStopAtRef = useRef(0);
  const chatRootRef = useRef<HTMLDivElement>(null);
  const chatFollowTailRef = useRef(true);

  const { setFocusedTab, selectTabByTab, addNotification, answerNotification } = useMultiplexer();
  const openPlanTarget = usePlanTargetNavigation();
  const tabSelector = useCallback((s: Parameters<typeof selectTab>[0]) => selectTab(s, tabId), [tabId]);
  const tab = useMuxSelector(tabSelector, tabFieldsEqual);
  const tabTitleMap = useMuxSelector(selectTabTitleMap, tabTitleMapEqual);
  const planNav = useMuxSelector(selectPlanNavContext, planNavContextEqual);
  const tabAgentTypes = useMuxSelector(selectTabAgentTypes, tabAgentTypesEqual);
  const askSelector = useCallback((s: Parameters<typeof selectTabAsks>[0]) => selectTabAsks(s, tabId), [tabId]);
  const asks = useMuxSelector(askSelector, tabAsksEqual);
  const { index: planIndex } = usePlanIndexOptional();
  const tabProvider = tab?.agent_type ?? null;
  const provider = useTabProvider(tab);
  const shellSessionModel = useShellSessionModel(tab);
  const tabModel = tab?.agent_session_model ?? shellSessionModel;
  const workingModelBadge = tabModel ? formatModelBadge(tabModel) : null;
  const agentSessionId = tab?.agent_session_id ?? null;
  const currentPtyId = tab?.pty_id ?? null;
  const backendState = tab?.agent_state ?? null;
  const sessionBindingSurface = useSessionBindingSurface(tabId, agentSessionId);
  const chatBacking = tab?.backing?.kind === "chat_backend" ? tab.backing : null;
  const effectiveChatBackend = chatBackend || chatBacking !== null;
  const watcherSpec = tab?.watcher ?? null;
  const isWatcherTab = watcherSpec != null;
  const pendingPrompt: PendingPrompt | null = tab?.pending_prompt ?? null;
  const autoConfirmEnabled = tab?.permission_auto_confirm === true;
  const autoConfirmations = autoConfirmationsForSession(tab?.permission_auto_confirmations, agentSessionId);
  const isActive = isVisible;
  const resolvePeerName = useMeshPeerNameResolver(isActive);

  const showComposerError = useCallback((message: string) => {
    addNotification({
      tab_id: tabId,
      workspace_id: null,
      title: isMicrophonePermissionError(message)
        ? "Microphone unavailable"
        : "Chat error",
      body: isMicrophonePermissionError(message)
        ? `${message} Open microphone permissions in Settings if needed.`
        : message,
      severity: "Error",
      level: "failure",
    });
  }, [addNotification, tabId]);

  const {
    loading,
    recovering,
    error,
    attachmentState,
    transcriptUnavailable,
    allMessages,
    enrichedMessages,
    hiddenMessageCount,
    canBackfillOlder,
    backfillLoading,
    loadOlder,
    showLatest,
    setHistoryReading,
    addOptimistic,
    updateOptimistic,
    observeChatSendStatus,
  } = useChatSession({
    tabId,
    agentSessionId,
    sessionBindingNonce: tab?.session_binding_nonce,
    onError,
    active: isActive,
    diagnosticSink: chatProjectionMonitor.sink,
    allowUnboundAuthority: effectiveChatBackend,
    followTailRef: chatFollowTailRef,
  });
  const bootSetupView = chatSetupView(tab, attachmentState === "attached");
  const hasTranscript = enrichedMessages.length > 0;
  // 'Loading chat' gates on the CHAT channel only (transcript authority + chat
  // reachability); remote PTY/output channel state never gates it.
  const remoteChatReconnecting = tab?.remote_channels?.chat === "reconnecting";
  const chatChannelLoading = loading || recovering || (!hasTranscript && remoteChatReconnecting);
  const chatLoading = chatChannelLoading || error != null;
  const setupView = chatBodyView({
    hasTranscript,
    chatLoading,
    unavailable: transcriptUnavailable,
    bootView: bootSetupView,
  });

  const {
    scrollRef,
    showScrollDown,
    scrollToBottom,
    followSend: rawFollowSentMessage,
    prepareForHistoryPrepend,
    handleScroll,
    revealMessage,
  } = useChatScroll({
    sessionKey: `${tabId}:${agentSessionId ?? ""}`,
    active: isActive,
    loading,
    messageCount: enrichedMessages.length,
    contentVersion: enrichedMessages,
    prependVersion: enrichedMessages[0]?.msg.id ?? null,
    followTailRef: chatFollowTailRef,
    onModeChange: (mode) => setHistoryReading(mode === "detached"),
  });
  useChatMessageReveal({
    tabId,
    ready: isActive && !chatChannelLoading,
    contentVersion: enrichedMessages,
    revealMessage,
  });

  // A detached reader stays on its content through a send; only a followed tail
  // re-projects the bounded window and moves with it.
  const followSentMessage = useCallback(() => {
    if (!chatFollowTailRef.current) return;
    showLatest();
    rawFollowSentMessage();
  }, [showLatest, rawFollowSentMessage]);

  const [answeredInteractiveIds, setAnsweredInteractiveIds] = useState<Set<string>>(new Set());
  const [bindingRecoveryOpen, setBindingRecoveryOpen] = useState(false);
  // clears tab.pending_prompt shortly after on the next state poll.
  const [answeredPromptQuestion, setAnsweredPromptQuestion] = useState<string | null>(null);

  useEffect(() => {
    if (!sessionBindingSurface.showRecoveryAction) setBindingRecoveryOpen(false);
  }, [sessionBindingSurface.showRecoveryAction]);
  const activePrompt = pendingPrompt && pendingPrompt.question !== answeredPromptQuestion ? pendingPrompt : null;
  const inlinePrompt = effectiveChatBackend ? activePrompt : null;

  useEffect(() => {
    if (!pendingPrompt) setAnsweredPromptQuestion(null);
  }, [pendingPrompt]);

  const handleRespondPrompt = useCallback(
    (value: string) => {
      if (pendingPrompt) setAnsweredPromptQuestion(pendingPrompt.question);
      void invoke("mux_respond_prompt", { tabId, value })
        .catch((e) => showComposerError(e instanceof Error ? e.message : String(e)));
    },
    [pendingPrompt, tabId, showComposerError],
  );

  const handleToggleAutoConfirm = useCallback(
    (enabled: boolean) => {
      void invoke("mux_set_permission_auto_confirm", { tabId, enabled })
        .catch((e) => showComposerError(e instanceof Error ? e.message : String(e)));
    },
    [tabId, showComposerError],
  );

  // input path the composer uses; jump surfaces the live terminal output.
  const handleRerunShellCommand = useCallback((command: string) => {
    if (!currentPtyId) {
      showComposerError("Tab has no PTY");
      return;
    }
    void invoke("write_pty_string", { ptyId: currentPtyId, data: command + "\r", raw: false }).catch(
      (e) => showComposerError(e instanceof Error ? e.message : String(e)),
    );
  }, [currentPtyId, showComposerError]);

  const handleJumpToShellCommand = useCallback(() => {
    onOpenTerminalFallback?.();
  }, [onOpenTerminalFallback]);

  const focusComposer = useCallback(() => {
    chatRootRef.current?.querySelector<HTMLTextAreaElement>("textarea")?.focus();
  }, []);

  const handleStopAgent = useCallback(() => {
    if (!currentPtyId) {
      showComposerError("Tab has no PTY");
      return;
    }
    const now = Date.now();
    if (now - lastStopAtRef.current < 2000) {
      return;
    }
    lastStopAtRef.current = now;
    void invoke("interrupt_agent", { ptyId: currentPtyId }).catch((e) =>
      showComposerError(e instanceof Error ? e.message : String(e)),
    );
  }, [currentPtyId, showComposerError]);

  const handleAnswerInteractive = useCallback<InteractiveAnswerHandler>(
    async (text, element) => {
      const answer = text.trim();
      if (!answer) return;
      if (!currentPtyId) {
        showComposerError("Tab has no PTY");
        throw new Error("Tab has no PTY");
      }
      setAnsweredInteractiveIds((prev) => new Set(prev).add(element.id));
      followSentMessage();
      addOptimistic(answer);
      try {
        await invoke("write_pty_string", { ptyId: currentPtyId, data: answer + "\r", raw: false });
      } catch (e) {
        setAnsweredInteractiveIds((prev) => {
          const next = new Set(prev);
          next.delete(element.id);
          return next;
        });
        showComposerError(e instanceof Error ? e.message : String(e));
        throw e;
      }
    },
    [currentPtyId, addOptimistic, followSentMessage, showComposerError],
  );

  const {
    attachments,
    addAttachments,
    removeAttachment,
    clearAttachments,
    handlePasteEvent,
    webDragOver,
    webDropHandlers,
  } = useAttachments({
    chatRootRef,
    enabled: !loading && !error,
    onError: showComposerError,
  });

  // Names change far more rarely than the transcript that carries them; keep the
  // map (and resolveTabName below) stable while streaming only appends content.
  const communicationTabNamesRef = useRef<{ key: string; value: Map<string, string> }>({
    key: "",
    value: new Map(),
  });
  const communicationTabNames = useMemo(() => {
    const names = new Map(tabTitleMap);
    for (const { origin } of enrichedMessages) {
      if (origin?.kind !== "tab") continue;
      const sender = origin.title.split("@")[0]?.trim();
      if (sender) names.set(origin.tabIdShort.slice(0, 8), sender);
    }
    const key = JSON.stringify([...names]);
    const prev = communicationTabNamesRef.current;
    if (key === prev.key) return prev.value;
    communicationTabNamesRef.current = { key, value: names };
    return names;
  }, [enrichedMessages, tabTitleMap]);

  const resolveTabName = useCallback(
    (tabRef: string): string | undefined => communicationTabNames.get(tabRef.slice(0, 8)),
    [communicationTabNames],
  );

  const reportProjection = useAgentReportProjection(tabId);
  // The sender's own transcript never contains the report it just sent, so the
  // outbound card comes from the accepted-report event, keyed by report_id.
  const outboundReports = useMemo<
    Array<{ targetTabId: string; at: number | null; descriptor: OutboundReportDescriptor }>
  >(
    () => reportProjection.outbound.map((report) => ({
      targetTabId: report.targetTabId,
      at: report.timestamp,
      descriptor: {
        reportId: report.reportId,
        recipient: tabTitleMap.get(report.targetTabId.slice(0, 8)) ?? report.targetTabId.slice(0, 8),
        task: report.task,
        status: report.status,
        sha: report.sha,
        message: report.message,
        taskOutcome: report.taskOutcome && {
          persisted: report.taskOutcome.persisted,
          transitionRejected: report.taskOutcome.transition_rejected,
          cause: report.taskOutcome.cause ?? null,
          reportOutcome: report.taskOutcome.report_outcome ?? null,
        },
      },
    })),
    [reportProjection, tabTitleMap],
  );

  const renderOutboundReport = (index: number) => {
    const entry = outboundReports[index];
    if (!entry) return null;
    return (
      <div
        key={entry.descriptor.reportId}
        data-testid="chat-outbound-report-row"
        data-report-id={entry.descriptor.reportId}
        role="article"
        aria-label={`Report to ${entry.descriptor.recipient}`}
      >
        <OutboundReportCard
          report={entry.descriptor}
          onFocusTab={() => resolveFocusTab(entry.targetTabId)}
          fontScale={uiFontScale}
          onOpenPlan={onOpenPlan}
          resolvePlanRef={resolvePlanRef}
        />
      </div>
    );
  };
  const renderAsk = (index: number) => {
    const ask = asks[index];
    if (!ask) return null;
    return (
      <ChatAskCard
        key={ask.id}
        notification={ask}
        onAnswer={answerNotification}
        resolveTabName={resolveTabName}
        fontScale={uiFontScale}
      />
    );
  };
  const renderedEnrichedMessages = useMemo(
    () => enrichedMessages.filter(({ msg }) => !isSessionIntentOnlyMessage(msg)),
    [enrichedMessages],
  );
  const sessionIntent = useMemo(() => sessionIntentFromMessages(allMessages), [allMessages]);
  // The rails are content, so they answer to the same authority the setup view
  // does: a body that still owns a loading or failed state emits no rail rows.
  const railsVisible = chatRailsVisible(setupView);
  const { reports: reportPlacement, asks: askPlacement } = useMemo(
    () => placeChatRails({
      visible: railsVisible,
      messages: renderedEnrichedMessages,
      messageTimeMs: ({ msg }) => messageTimeMs(msg.timestamp),
      reportTimes: outboundReports.map((report) => report.at),
      askTimes: asks.map((ask) => normalizeEpochMs(ask.timestamp)),
    }),
    [asks, outboundReports, railsVisible, renderedEnrichedMessages],
  );
  const deferredPlanRefMessages = useDeferredValue(enrichedMessages);
  // Record the visible tail after paint so diagnostics do not overstate progress
  // when React commits faster than the browser paints.
  useEffect(() => {
    const tail = paintTailSignature(renderedEnrichedMessages[renderedEnrichedMessages.length - 1]?.msg);
    return scheduleAfterPaint(() => chatProjectionMonitor.recordPaint(tabId, tail));
  }, [tabId, renderedEnrichedMessages]);
  // Drop torn-down tab diagnostics immediately instead of waiting for LRU eviction.
  useEffect(() => () => chatProjectionMonitor.forget(tabId), [tabId]);
  const latestFailedToolCallId = useMemo(() => {
    for (let i = renderedEnrichedMessages.length - 1; i >= 0; i -= 1) {
      const msg = renderedEnrichedMessages[i]?.msg;
      if (msg?.type === "tool_call" && msg.toolError === true) {
        return msg.id;
      }
    }
    return null;
  }, [renderedEnrichedMessages]);

  // Keep the ref array stable when streaming changes no plan reference.
  const visiblePlanRefsRef = useRef<{
    key: string;
    value: Array<{ planId: string; targetId?: string; targetKind: "plan" | "task" | "section" }>;
  }>({ key: "", value: [] });
  const visiblePlanRefs = useMemo(() => {
    const refs: Array<{ planId: string; targetId?: string; targetKind: "plan" | "task" | "section" }> = [];
    for (const { msg } of deferredPlanRefMessages) {
      if (msg.type === "tool_result" || msg.type === "system" || msg.type === "local_command") continue;
      for (const ref of extractPlanRefs(msg.content ?? "")) {
        refs.push({
          planId: ref.planId,
          targetId: ref.targetId,
          targetKind: ref.targetKind,
        });
      }
    }
    const key = refs.map((r) => `${r.targetKind}:${r.planId}:${r.targetId ?? ""}`).join("|");
    const prev = visiblePlanRefsRef.current;
    if (key === prev.key) return prev.value;
    visiblePlanRefsRef.current = { key, value: refs };
    return refs;
  }, [deferredPlanRefMessages]);
  const resolvedPlanRefs = usePlanRefResolver(
    api ?? DISABLED_PLAN_API,
    visiblePlanRefs,
  );
  const resolvePlanRef = useCallback(
    (planId: string, targetId?: string, targetKind?: "plan" | "task" | "section") => {
      const kind = targetKind ?? (targetId ? "task" : "plan");
      const key = `${kind}:${planId}:${targetId ?? ""}`;
      const found = resolvedPlanRefs.refs.get(key);
      if (found) {
        return chatPlanRefView(
          found,
          planIndex,
          planNav.workspaces,
          resolvedPlanRefs.unreachable,
        );
      }
      return {
        title: targetId ?? planId,
        planTitle: planId,
        state: resolvedPlanRefs.unreachable ? "unreachable" as const : "loading" as const,
        rawLabel: targetId ?? planId,
        fullRef: targetId ? `${planId}/${targetId}` : planId,
      };
    },
    [resolvedPlanRefs, planIndex, planNav.workspaces],
  );

  const onOpenPlan = useCallback(
    (planId: string, taskId?: string, targetKind?: "plan" | "task" | "section") => {
      const kind = targetKind ?? (taskId ? "task" : "plan");
      const resolved = resolvedPlanRefs.refs.get(`${kind}:${planId}:${taskId ?? ""}`);
      openPlanTarget({
        planId,
        taskId: kind === "task" ? taskId : undefined,
        canonicalProjectPath: resolved?.canonical_project_path,
      });
    },
    [openPlanTarget, resolvedPlanRefs.refs],
  );

  const resolveFocusTab = useCallback((ref: string) => {
    const target = resolveTabNavigationTarget(ref, Object.keys(tabAgentTypes), planNav.workspaces);
    if (target) void selectTabByTab(target.workspaceId, target.tabId);
  }, [tabAgentTypes, planNav.workspaces, selectTabByTab]);

  // Preserve origin prop identities so streamed chunks do not re-render old bubbles.
  type OriginProps = { key: string; onOriginClick: () => void; originAvatar: ReactNode };
  const originPropsCacheRef = useRef<Map<string, OriginProps>>(new Map());
  const originFocusRef = useRef<((tabId: string) => void) | null>(null);
  const originPropsById = useMemo(() => {
    // onOriginClick closes over setFocusedTab, so a new setFocusedTab identity
    // must rebind every entry — drop the cache instead of reusing stale closures.
    if (originFocusRef.current !== setFocusedTab) {
      originFocusRef.current = setFocusedTab;
      originPropsCacheRef.current = new Map();
    }
    const cache = originPropsCacheRef.current;
    const next = new Map<string, OriginProps>();
    for (const { msg, origin } of renderedEnrichedMessages) {
      if (origin?.kind !== "tab") continue;
      const idShort = origin.tabIdShort;
      const fullId = Object.keys(tabAgentTypes).find((id) => id.startsWith(idShort));
      if (!fullId) continue;
      const providerId = tabAgentTypes[fullId] ?? "";
      const key = `${fullId}:${providerId}`;
      const prev = cache.get(msg.id);
      const entry: OriginProps = prev && prev.key === key
        ? prev
        : {
            key,
            onOriginClick: () => setFocusedTab(fullId),
            originAvatar: providerId ? <OriginProviderAvatar providerId={providerId} /> : undefined,
          };
      next.set(msg.id, entry);
    }
    originPropsCacheRef.current = next;
    return next;
  }, [renderedEnrichedMessages, tabAgentTypes, setFocusedTab]);

  // subtype); it arrives via the mux snapshot and events, no local polling needed.
  const liveAgentState: AgentState = useMemo(() => {
    const kind = backendState?.state ?? "idle";
    return {
      state: kind,
      subtype: backendState?.subtype,
      status_line: backendState?.status_line,
      phase: backendState?.phase,
    };
  }, [
    backendState?.state,
    backendState?.status_line,
    backendState?.phase,
    backendState?.subtype,
  ]);

  const composerModelBubble = useMemo(() => {
    if (!effectiveChatBackend || !chatBacking) return undefined;
    return (
      <>
        <ChatModelBubble pluginId={chatBacking.plugin_id} sessionId={chatBacking.session_id} />
        <ChatPresetBubble pluginId={chatBacking.plugin_id} sessionId={chatBacking.session_id} />
      </>
    );
  }, [chatBacking, effectiveChatBackend]);

  const watcherModelBubble = useMemo(() => {
    if (!effectiveChatBackend || !chatBacking) return undefined;
    return (
      <>
        <ChatModelBubble pluginId={chatBacking.plugin_id} sessionId={chatBacking.session_id} readOnly />
        <ChatPresetBubble pluginId={chatBacking.plugin_id} sessionId={chatBacking.session_id} readOnly />
      </>
    );
  }, [chatBacking, effectiveChatBackend]);

  const statusRowState: AgentState = useMemo(() => {
    if (transcriptUnavailable) return { state: "idle", status_line: "Chat transcript unavailable" };
    if (error) return { state: "idle", status_line: "Chat unavailable" };
    if (chatChannelLoading) return { state: "working", status_line: "Loading chat…" };
    if (!hasTranscript && bootSetupView.kind === "starting") {
      return {
        state: "working",
        status_line: chatSetupStageLabel(
          tab?.boot_stage,
          chatSetupSubject(tab, provider?.displayName ?? "the agent"),
        ),
      };
    }
    if (sessionBindingSurface.agentState && liveAgentState.state !== "question") {
      return sessionBindingSurface.agentState;
    }
    return liveAgentState;
  }, [bootSetupView.kind, chatChannelLoading, error, hasTranscript, liveAgentState, provider?.displayName, sessionBindingSurface.agentState, tab, transcriptUnavailable]);
  const showEmptyState = setupView.kind === "none"
    && !chatLoading
    && renderedEnrichedMessages.length === 0
    && outboundReports.length === 0
    && autoConfirmations.length === 0
    && !inlinePrompt;

  return (
    <div
      ref={chatRootRef}
      className="w-full h-full flex flex-col relative"
      data-testid="tab-session-chat"
      data-tab-id={tabId}
      role="region"
      aria-label={`Tab chat: ${tab?.title ?? tabId}`}
      aria-busy={chatChannelLoading && setupView.kind !== "failed"}
      style={{
        background: "var(--ct-bg, #0d1117)",
        color: "var(--ct-fg, #c9d1d9)",
      }}
      {...webDropHandlers}
    >
      {webDragOver && (
        <div
          data-testid="web-drop-overlay"
          className="absolute inset-0 z-20 flex items-center justify-center border-2 border-dashed pointer-events-none"
          style={{
            borderColor: "var(--ct-accent, #58a6ff)",
            background: "color-mix(in srgb, var(--ct-bg, #0d1117) 70%, transparent)",
          }}
        >
          <span className="text-sm font-mono">Drop files to attach</span>
        </div>
      )}
      <ChatSessionIntentLine intent={sessionIntent} />
      <div className="relative flex-1 min-h-0">
      {showEmptyState && (
        <div
          className="chat-empty-state pointer-events-none absolute inset-0 flex items-center justify-center select-none"
          data-testid="chat-empty-state"
          aria-hidden
        >
          <span className="chat-empty-mark" aria-hidden>
            <AgentIcon providerId={tabProvider} className="chat-empty-glyph text-[56px]" />
          </span>
        </div>
      )}
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="absolute inset-0 overflow-y-auto overscroll-contain p-3 select-text"
        data-testid="tab-session-chat-messages"
        role="log"
        aria-label="Conversation"
        aria-live="polite"
      >
        <div>
        {touchMode && (
          <div
            aria-hidden="true"
            className="shrink-0"
            style={{ height: "calc(1.25rem + var(--ct-safe-top, 0px))" }}
          />
        )}
        {(hiddenMessageCount > 0 || canBackfillOlder) && (
          <div className="flex justify-center pb-2 pt-1">
            <button
              type="button"
              data-testid="chat-load-older"
              onClick={() => {
                prepareForHistoryPrepend();
                void loadOlder();
              }}
              disabled={backfillLoading}
              className="rounded px-2.5 py-1 text-xs font-mono hover:brightness-125 disabled:opacity-50"
              style={{
                background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
                color: "var(--ct-muted-fg)",
                border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 25%, transparent)",
              }}
            >
              {hiddenMessageCount > 0
                ? `Show older (${hiddenMessageCount})`
                : backfillLoading
                  ? "Loading older…"
                  : "Show older"}
            </button>
          </div>
        )}
        {(reportPlacement.afterIndex.get(-1) ?? []).map(renderOutboundReport)}
        {(askPlacement.afterIndex.get(-1) ?? []).map(renderAsk)}
        {renderedEnrichedMessages.map(({ msg, item, origin }, messageIndex) => {
          const originProps = originPropsById.get(msg.id);
          const following = reportPlacement.afterIndex.get(messageIndex) ?? [];
          return (
            <Fragment key={msg.id}>
            <div
              key={msg.id}
              data-testid="chat-message"
              data-message-role={msg.type}
              data-message-id={msg.id}
              data-source-message-id={msg.sourceMessageId ?? msg.id}
              role="article"
              aria-label={chatMessageAccessibleName(msg.type, msg.content)}
            >
              <MessageBubble
                message={msg}
                item={item.kind === "tool" ? item : undefined}
                isLatestFailedToolCall={msg.id === latestFailedToolCallId}
                origin={origin}
                resolveTabName={resolveTabName}
                resolvePeerName={resolvePeerName}
                onOpenPlan={onOpenPlan}
                resolvePlanRef={resolvePlanRef}
                fontScale={uiFontScale}
                onAnswerInteractive={handleAnswerInteractive}
                answeredInteractiveIds={answeredInteractiveIds}
                onRerunShellCommand={handleRerunShellCommand}
                onJumpToShellCommand={onOpenTerminalFallback ? handleJumpToShellCommand : undefined}
                originAvatar={originProps?.originAvatar}
                onOriginClick={originProps?.onOriginClick}
                onFocusTab={resolveFocusTab}
              />
            </div>
              {following.map(renderOutboundReport)}
              {(askPlacement.afterIndex.get(messageIndex) ?? []).map(renderAsk)}
            </Fragment>
          );
        })}
        {reportPlacement.trailing.map(renderOutboundReport)}
        {askPlacement.trailing.map(renderAsk)}
        <ChatSetupState
          tab={tab}
          view={setupView}
          providerName={provider?.displayName ?? "the agent"}
          providerIconSvg={provider?.iconSvg}
          providerIconColor={provider?.iconColor}
          providerIconHint={provider?.iconHint}
          onEscape={focusComposer}
        />
        <AutoConfirmedPromptRows records={autoConfirmations} />
        <PendingPromptCard pendingPrompt={inlinePrompt} onRespond={handleRespondPrompt} />
        </div>
      </div>
      {showScrollDown && (
        <ScrollToLatestButton
          testId="chat-scroll-bottom"
          onClick={() => {
            showLatest();
            scrollToBottom();
          }}
          title="Scroll to latest"
          className="absolute bottom-4 right-4 z-30"
        />
      )}
      </div>
      {isWatcherTab && watcherSpec ? (
        <WatcherReadOnlyBar
          watcher={watcherSpec}
          modelBubble={watcherModelBubble}
        />
      ) : (
      <>
      <div className="w-full px-3">
        <AgentStateIndicator
          state={statusRowState}
          providerName={provider?.displayName ?? tabProvider}
          model={workingModelBadge}
          className="mx-auto w-full max-w-4xl"
          onStop={effectiveChatBackend || !currentPtyId ? undefined : handleStopAgent}
          onCancel={effectiveChatBackend || !currentPtyId ? undefined : handleStopAgent}
          onOpenTerminal={effectiveChatBackend ? undefined : onOpenTerminalFallback}
          onRebind={sessionBindingSurface.showRecoveryAction
            ? () => setBindingRecoveryOpen(true)
            : undefined}
        />
        {tabProvider && !effectiveChatBackend && (
          <div className="mx-auto flex w-full max-w-4xl justify-end pt-1">
            <PermissionAutoConfirmToggle enabled={autoConfirmEnabled} onToggle={handleToggleAutoConfirm} />
          </div>
        )}
        {bindingRecoveryOpen && (
          <SessionBindingRecoveryForm
            tabId={tabId}
            suggestedSessionId={sessionBindingSurface.rebindSessionId}
            onClose={() => setBindingRecoveryOpen(false)}
          />
        )}
      </div>
      <ChatComposer
        tabId={tabId}
        tabProvider={tabProvider}
        currentPtyId={currentPtyId}
        chatBackend={effectiveChatBackend}
        isActive={isActive}
        pendingPrompt={pendingPrompt}
        attachments={attachments}
        addAttachments={addAttachments}
        removeAttachment={removeAttachment}
        clearAttachments={clearAttachments}
        handlePasteEvent={handlePasteEvent}
        onWillSendMessage={followSentMessage}
        addOptimistic={addOptimistic}
        updateOptimistic={updateOptimistic}
        observeChatSendStatus={observeChatSendStatus}
        showComposerError={showComposerError}
        onOpenTerminalFallback={effectiveChatBackend ? undefined : onOpenTerminalFallback}
        modelBubble={composerModelBubble}
      />
      </>
      )}
    </div>
  );
}
