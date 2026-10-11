import { CHAT_ERROR_TOAST_LIFETIME_MS } from "../Notifications/toastLifetime";
import { LocalArtifactScope } from "@codeterm/chat-ui";
import { documentsFromTranscript } from "./chatDocuments";
import { chatDocumentsStore } from "./chatDocumentsStore";
import { chatChangesStore } from "./chatChangesStore";
import { RemovedChatPlugin } from "./RemovedChatPlugin";
import { Fragment, useCallback, useDeferredValue, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { invoke } from "../../lib/bridge";
import {
  ChatSessionIntentLine,
  isSessionIntentOnlyMessage,
  MessageBubble,
  MessageTimeFrame,
  OutboundReportCard,
  sessionIntentFromMessages,
} from "@codeterm/chat-ui";
import { ChatBlockHostProvider, PluginNameProvider, extractPlanRefs } from "@codeterm/chat-ui";
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
import { getAgentDescriptor } from "../../lib/agentDescriptorCache";
import { formatModelBadge } from "../../lib/formatModelBadge";
import { statusLineModelName } from "./statusLinePickerModel";
import { usePlanIndexOptional } from "../Canvas/PlansProvider";
import { usePlanTargetNavigation } from "../Canvas/teamViewNavigation";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import type { ApiConfig, PendingPrompt } from "../../lib/types";
import { ChatComposer } from "./ChatComposer";
import { ManagedTabBar } from "./ManagedTabBar";
import { managedTabPresentation } from "../../lib/managedTab";
import { ComposerApprovalDock } from "./ComposerApprovalDock";
import { TabLoadingIndicator } from "../shared/TabLoadingIndicator";
import { PendingPromptCard } from "./PendingPromptCard";
import { AutoConfirmedPromptRows, autoConfirmationsForSession } from "./PermissionAutoConfirm";
import { AgentStateIndicator } from "./AgentStateIndicator";
import { TabStatusLineModelPicker } from "./StatusLineModelPicker";
import { useTabActivity } from "../../lib/useTabActivity";
import { useAttachments } from "./useAttachments";
import { useChatSession, isCodeTermFrontendTestMode, paintTailSignature } from "./useChatSession";
import { chatProjectionMonitor, scheduleAfterPaint } from "./chatProjectionDiagnostics";
import { ChatModelBubble } from "../Extensions/ChatModelBubble";
import { ChatPresetBubble } from "../Extensions/ChatPresetBubble";
import { PluginIcon } from "../Extensions/PluginIcon";
import { getPluginProviderMeta, pluginProviderMetaVersion, subscribePluginProviderMeta } from "../../lib/pluginProviderMeta";
import { usePluginDisplayNameResolver } from "../../hooks/usePluginProviderMeta";
import type { ChatOrigin } from "../../lib/chatPrefixes";
import { WatcherReadOnlyBar } from "./WatcherReadOnlyBar";
import { ScrollToLatestButton } from "./ScrollToLatestButton";
import { ChatSetupState, chatSetupView } from "./ChatSetupState";
import { usePlanRefResolver } from "./usePlanRefResolver";
import { chatPlanRefView } from "./chatPlanRef";
import { useChatLayoutInsets } from "./useChatLayoutInsets";
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
import { BlueprintChatCards } from "./BlueprintChatCards";
import { useTabChatBlockHost } from "./pinnedBlocks";
import { interruptAgentArgs } from "../../lib/agentInterrupt";
import { useTabChoiceHost } from "./useTabChoiceHost";
import { ChatMessageRow } from "./ChatMessageRow";
import { ChatDeliveryState } from "./ChatDeliveryState";
import { deliveryState, sendForRow } from "./chatDeliveryModel";
import { beginChatSendRetry, failChatSendRequest, useChatSends } from "./chatSendStore";
import { sendTrackedChatMessage } from "./chatSendStatus";
import { prepareChatDelivery } from "./chatDeliveryPreparation";
import { useDsTasksExpander } from "./dsTasks/useDsTasksExpander";
import { MessageSelectionBar } from "./MessageSelectionBar";
import { useChatMessageSelection } from "./useChatMessageSelection";
import { streamingMessageId } from "./streamingMessage";
import { jumpToMessage } from "./jumpToMessage";
import { HelperMomentCard } from "./HelperMomentCard";
import { helperMoments } from "../../lib/helperMoments/helperMomentsWiring";
import { latestAnswerText } from "../../lib/helperMoments/helperBlocks";
import { isGeneralAgent } from "../../lib/roleKind";

interface Props {
  inputStatus?: ReactNode;
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

// Runs for every row on every streamed chunk; only the head of a long message can reach the excerpt.
const ACCESSIBLE_NAME_SCAN_CHARS = 1_000;

function chatMessageAccessibleName(type: string, content: string): string {
  const head = content.length > ACCESSIBLE_NAME_SCAN_CHARS ? content.slice(0, ACCESSIBLE_NAME_SCAN_CHARS) : content;
  let compact = head.replace(/\s+/g, " ").trim();
  if (compact.length <= 80 && head.length < content.length) compact = content.replace(/\s+/g, " ").trim();
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

function OriginPluginAvatar({ pluginId, iconSvg }: { pluginId: string; iconSvg?: string | null }) {
  const meta = getPluginProviderMeta(pluginId);
  const resolvedIcon = iconSvg ?? meta?.iconSvg;
  return (
    <span
      className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-full border"
      style={{
        background: "color-mix(in srgb, var(--ct-surface, #161b22) 86%, var(--ct-accent, #58a6ff) 14%)",
        borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 28%, transparent)",
        color: "var(--ct-fg)",
      }}
      aria-hidden="true"
    >
      <PluginIcon plugin={{ icon_svg: resolvedIcon, icon_color: meta?.iconColor }} size={14} />
    </span>
  );
}

export function TabSessionChat({
  inputStatus,
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
  const deliverySends = useChatSends(tabId);
  const { expand: expandDeliveryTasks, plansSink: deliveryPlansSink } = useDsTasksExpander(tabId);
  const tabBlockHost = useTabChatBlockHost(tabId);
  const lastStopAtRef = useRef(0);
  const chatRootRef = useRef<HTMLDivElement>(null);
  const floatingBottomInset = useChatLayoutInsets(chatRootRef);
  const chatFollowTailRef = useRef(true);

  const { setFocusedTab, selectTabByTab, addNotification, answerNotification } = useMultiplexer();
  const openPlanTarget = usePlanTargetNavigation();
  const tabSelector = useCallback((s: Parameters<typeof selectTab>[0]) => selectTab(s, tabId), [tabId]);
  const tab = useMuxSelector(tabSelector, tabFieldsEqual);
  const workspaceSelector = useCallback((s: Parameters<typeof selectTab>[0]) => s.workspaces.find((workspace) => workspace.tabs.includes(tabId))?.id ?? s.tabs[tabId]?.workspace_id ?? null, [tabId]);
  const workspaceId = useMuxSelector(workspaceSelector);
  const tabTitleMap = useMuxSelector(selectTabTitleMap, tabTitleMapEqual);
  const planNav = useMuxSelector(selectPlanNavContext, planNavContextEqual);
  const tabAgentTypes = useMuxSelector(selectTabAgentTypes, tabAgentTypesEqual);
  const askSelector = useCallback((s: Parameters<typeof selectTabAsks>[0]) => selectTabAsks(s, tabId), [tabId]);
  const asks = useMuxSelector(askSelector, tabAsksEqual);
  const { index: planIndex } = usePlanIndexOptional();
  const tabProvider = tab?.agent_type ?? null;
  const provider = useTabProvider(tab);
  const pluginMetaVersion = useSyncExternalStore(
    subscribePluginProviderMeta,
    pluginProviderMetaVersion,
    pluginProviderMetaVersion,
  );
  const shellSessionModel = useShellSessionModel(tab);
  const tabModel = tab?.agent_session_model ?? shellSessionModel;
  const workingModelBadge = formatModelBadge(tab ? statusLineModelName(tab, shellSessionModel) : tabModel ?? "Unknown model");
  const agentSessionId = tab?.agent_session_id ?? null;
  const currentPtyId = tab?.pty_id ?? null;
  const agentActivity = useTabActivity(tabId);
  const sessionBindingSurface = useSessionBindingSurface(tabId, agentSessionId);
  const chatBacking = tab?.backing?.kind === "chat_backend" ? tab.backing : null;
  const effectiveChatBackend = chatBackend || chatBacking !== null;
  const watcherSpec = tab?.watcher ?? null;
  const isWatcherTab = watcherSpec != null;
  const pendingPrompt: PendingPrompt | null = tab?.pending_prompt ?? null;
  const structuredPrompts = getAgentDescriptor(tabProvider)?.supports_structured_prompts === true;
  const autoConfirmations = autoConfirmationsForSession(tab?.permission_auto_confirmations, agentSessionId);
  const isActive = isVisible;
  const resolvePeerName = useMeshPeerNameResolver(isActive);
  const resolvePluginName = usePluginDisplayNameResolver();

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
      ephemeral: true,
      lifetime_ms: CHAT_ERROR_TOAST_LIFETIME_MS,
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
    newerMessageCount,
    canBackfillOlder,
    backfillLoading,
    loadOlder,
    showLatest,
    prepareMessage,
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
  const retryChatDelivery = async (id: string) => {
    const send = beginChatSendRetry(tabId, id);
    if (!send) return;
    let content = send.content;
    try { content = await prepareChatDelivery(tabId, send, expandDeliveryTasks); }
    catch { failChatSendRequest(tabId, id); return; }
    try {
      observeChatSendStatus(await sendTrackedChatMessage(tabId, send.correlationId ?? send.id, content));
    } catch {
      void invoke<import("./chatSendStatus").ChatSendStatus[]>("chat_send_status", { tabId })
        .then((statuses) => statuses.forEach(observeChatSendStatus)).catch(() => {});
    }
  };
  useEffect(() => {
    chatChangesStore.bind(tabId, JSON.stringify([agentSessionId, tab?.session_binding_nonce]));
  }, [tabId, agentSessionId, tab?.session_binding_nonce]);
  useEffect(() => {
    if (!isActive) return;
    const documents = documentsFromTranscript(allMessages);
    chatDocumentsStore.set(tabId, documents);
    chatChangesStore.set(tabId, allMessages);
  }, [tabId, allMessages, isActive]);
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
    handleContentLayout,
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
    prepareMessage,
  });

  const followSentMessage = useCallback(() => {
    showLatest();
    rawFollowSentMessage();
  }, [showLatest, rawFollowSentMessage]);
  const choiceHost = useTabChoiceHost(tabId, { messages: allMessages, addOptimistic, followSentMessage, observeChatSendStatus });
  const chatBlockHost = useMemo(() => ({ ...tabBlockHost, choice: choiceHost }), [tabBlockHost, choiceHost]);

  const generalAgent = isGeneralAgent(tab);
  const willSendMessage = useCallback((text: string) => {
    followSentMessage();
    if (generalAgent) void helperMoments.observe({ kind: "user-message", text, at: Date.now() });
  }, [followSentMessage, generalAgent]);

  const [answeredInteractiveIds, setAnsweredInteractiveIds] = useState<Set<string>>(new Set());
  const [bindingRecoveryOpen, setBindingRecoveryOpen] = useState(false);
  const [contextSlot, setContextSlot] = useState<HTMLDivElement | null>(null);
  const [answeredPromptQuestion, setAnsweredPromptQuestion] = useState<string | null>(null);
  useEffect(() => {
    if (!sessionBindingSurface.showRecoveryAction) setBindingRecoveryOpen(false);
  }, [sessionBindingSurface.showRecoveryAction]);
  const activePrompt = pendingPrompt && (structuredPrompts || pendingPrompt.question !== answeredPromptQuestion) ? pendingPrompt : null;
  const inlinePrompt = activePrompt;

  useEffect(() => {
    if (!pendingPrompt) setAnsweredPromptQuestion(null);
  }, [pendingPrompt]);

  const promptResponseInFlight = useRef(false);
  const handleRespondPrompt = useCallback(
    async (value: string) => {
      if (promptResponseInFlight.current) return;
      promptResponseInFlight.current = true;
      if (pendingPrompt && !structuredPrompts) setAnsweredPromptQuestion(pendingPrompt.question);
      try {
        await invoke("mux_respond_prompt", { tabId, value });
      } catch (error) {
        showComposerError(error instanceof Error ? error.message : String(error));
        throw error;
      } finally {
        promptResponseInFlight.current = false;
      }
    },
    [pendingPrompt, structuredPrompts, tabId, showComposerError],
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
  }, [currentPtyId, showComposerError, tabId, tab?.managed]);

  const handleJumpToShellCommand = useCallback(() => {
    onOpenTerminalFallback?.();
  }, [onOpenTerminalFallback]);

  const focusComposer = useCallback(() => {
    chatRootRef.current?.querySelector<HTMLTextAreaElement>("textarea")?.focus();
  }, []);

  const handleStopAgent = useCallback(() => {
    const args = interruptAgentArgs({ id: tabId, pty_id: currentPtyId, managed: tab?.managed });
    if (!args) {
      showComposerError("Tab has no PTY");
      return;
    }
    const now = Date.now();
    if (now - lastStopAtRef.current < 2000) {
      return;
    }
    lastStopAtRef.current = now;
    void invoke("interrupt_agent", args).catch((e) =>
      showComposerError(e instanceof Error ? e.message : String(e)),
    );
  }, [currentPtyId, showComposerError, tabId, tab?.managed]);

  const openJobs = useCallback(() => {
    window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:jobs" } }));
  }, []);

  const handleStopReminder = useCallback(async (id: string) => {
    try {
      await invoke("delayed_send_delete", { id });
    } catch (error) {
      showComposerError(error instanceof Error ? error.message : String(error));
    }
  }, [showComposerError]);

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
    uploadStatus,
    attachments,
    addAttachments,
    removeAttachment,
    clearAttachments,
    handlePasteEvent,
    inlineSinkRef,
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
        <MessageTimeFrame timestamp={entry.at == null ? undefined : String(entry.at)} side="left">
          <OutboundReportCard
            report={entry.descriptor}
            onFocusTab={() => resolveFocusTab(entry.targetTabId)}
            fontScale={uiFontScale}
            onOpenPlan={onOpenPlan}
            resolvePlanRef={resolvePlanRef}
          />
        </MessageTimeFrame>
      </div>
    );
  };
  const renderAsk = (index: number) => {
    const ask = asks[index];
    if (!ask) return null;
    return (
      <MessageTimeFrame key={ask.id} timestamp={String(ask.timestamp)} side="left">
        <ChatAskCard
          notification={ask}
          onAnswer={answerNotification}
          resolveTabName={resolveTabName}
          fontScale={uiFontScale}
        />
      </MessageTimeFrame>
    );
  };
  const renderedEnrichedMessages = useMemo(
    () => enrichedMessages.filter(({ msg }) => !isSessionIntentOnlyMessage(msg)),
    [enrichedMessages],
  );
  const streamingId = streamingMessageId(renderedEnrichedMessages, agentActivity.active);
  const messageSelection = useChatMessageSelection({ tabId, rows: renderedEnrichedMessages, agent: tab?.title || provider?.displayName || "Agent", surfaceRef: scrollRef, streamingMessageId: streamingId });
  const jumpToQuoted = useCallback((messageId: string) => { jumpToMessage(scrollRef.current, messageId); }, [scrollRef]);

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
  type OriginProps = { key: string; origin?: ChatOrigin; onOriginClick?: () => void; originAvatar?: ReactNode };
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
      if (origin?.kind === "plugin") {
        const meta = getPluginProviderMeta(origin.pluginId);
        const iconSvg = origin.iconSvg ?? meta?.iconSvg;
        const name = origin.name || resolvePluginName(origin.pluginId);
        const key = `plugin:${origin.pluginId}:${name ?? ""}:${iconSvg ?? "fallback"}:${meta?.iconColor ?? ""}`;
        const prev = cache.get(msg.id);
        const entry: OriginProps = prev && prev.key === key
          ? prev
          : {
              key,
              origin: name === origin.name ? origin : { ...origin, name },
              originAvatar: <OriginPluginAvatar pluginId={origin.pluginId} iconSvg={iconSvg} />,
            };
        next.set(msg.id, entry);
        continue;
      }
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
  }, [pluginMetaVersion, renderedEnrichedMessages, resolvePluginName, tabAgentTypes, setFocusedTab]);

  const watcherModelBubble = useMemo(() => {
    if (!effectiveChatBackend || !chatBacking) return undefined;
    return (
      <>
        <ChatModelBubble pluginId={chatBacking.plugin_id} sessionId={chatBacking.session_id} readOnly />
        <ChatPresetBubble pluginId={chatBacking.plugin_id} sessionId={chatBacking.session_id} readOnly />
      </>
    );
  }, [chatBacking, effectiveChatBackend]);

  const helperAnswer = useMemo(
    () => (generalAgent ? latestAnswerText(renderedEnrichedMessages.map(({ msg }) => msg)) : null),
    [generalAgent, renderedEnrichedMessages],
  );
  const showEmptyState = setupView.kind === "none"
    && !chatLoading
    && renderedEnrichedMessages.length === 0
    && outboundReports.length === 0
    && autoConfirmations.length === 0
    && !inlinePrompt;

  return (
    <LocalArtifactScope tabId={tabId}>
    <ChatBlockHostProvider value={chatBlockHost}>
    <PluginNameProvider resolve={resolvePluginName}>
    <div
      ref={chatRootRef}
      className="ct-chat-shell w-full h-full flex flex-col relative"
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
      <RemovedChatPlugin error={effectiveChatBackend ? error : null} />
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
        onKeyDown={messageSelection.onKeyDown}
        data-selecting={messageSelection.active || undefined}
        className="ct-chat-surface absolute inset-0 overflow-y-auto overscroll-contain px-5 py-3 select-text"
        data-testid="tab-session-chat-messages"
        role="log"
        aria-label="Conversation"
        aria-live="polite"
      >
        <div style={{ paddingBottom: floatingBottomInset }}>
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
              aria-label="Show older messages"
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
                  ? <TabLoadingIndicator label="Loading older messages" compact deferred={false} />
                  : "Show older"}
            </button>
          </div>
        )}
        {(reportPlacement.afterIndex.get(-1) ?? []).map(renderOutboundReport)}
        {(askPlacement.afterIndex.get(-1) ?? []).map(renderAsk)}
        <BlueprintChatCards tabId={tabId} workspaceId={workspaceId} />
        {renderedEnrichedMessages.map(({ msg, item, origin }, messageIndex) => {
          const originProps = originPropsById.get(msg.id);
          const following = reportPlacement.afterIndex.get(messageIndex) ?? [];
          const nextId = renderedEnrichedMessages[messageIndex + 1]?.msg.id;
          const deliverySend = sendForRow(deliverySends, msg);
          return (
            <Fragment key={msg.id}>
            <ChatMessageRow
              msg={msg}
              ariaLabel={chatMessageAccessibleName(msg.type, msg.content)}
              selected={messageSelection.selected.has(msg.id)}
              joinNext={nextId !== undefined && following.length === 0 && messageSelection.selected.has(nextId)}
              selecting={messageSelection.active}
              cursor={messageSelection.cursor ? messageSelection.cursor === msg.id : messageIndex === renderedEnrichedMessages.length - 1}
              handlers={messageSelection.rowHandlers}
            >
              <MessageBubble
                message={msg}
                onContentLayout={handleContentLayout}
                streaming={msg.id === streamingId}
                userFooter={deliverySend && deliveryState(deliverySend) !== "delivered"
                  ? <ChatDeliveryState state={deliveryState(deliverySend)} onRetry={deliverySend.status?.system_send ? undefined : () => void retryChatDelivery(deliverySend.id)} />
                  : undefined}
                item={item.kind === "tool" ? item : undefined}
                isLatestFailedToolCall={msg.id === latestFailedToolCallId}
                origin={originProps?.origin ?? origin}
                resolveTabName={resolveTabName}
                resolvePeerName={resolvePeerName}
                onOpenPlan={onOpenPlan}
                resolvePlanRef={resolvePlanRef}
                fontScale={uiFontScale}
                onAnswerInteractive={handleAnswerInteractive}
                answeredInteractiveIds={answeredInteractiveIds}
                onRerunShellCommand={handleRerunShellCommand}
                onStopReminder={handleStopReminder}
                onOpenJobs={openJobs}
                onJumpToShellCommand={onOpenTerminalFallback ? handleJumpToShellCommand : undefined}
                originAvatar={originProps?.originAvatar}
                onOriginClick={originProps?.onOriginClick}
                onFocusTab={resolveFocusTab}
                onJumpToMessage={jumpToQuoted}
              />
            </ChatMessageRow>
              {following.map(renderOutboundReport)}
              {(askPlacement.afterIndex.get(messageIndex) ?? []).map(renderAsk)}
            </Fragment>
          );
        })}
        {reportPlacement.trailing.map(renderOutboundReport)}
        {askPlacement.trailing.map(renderAsk)}
        {generalAgent && <HelperMomentCard idle={agentActivity.kind === "idle"} latestAnswer={helperAnswer} />}
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
        </div>
      </div>
      <MessageSelectionBar
        count={messageSelection.count}
        partial={messageSelection.partial}
        onCopy={messageSelection.copySelection}
        onQuote={messageSelection.quoteSelection}
        onAsk={messageSelection.askSelection}
        onClear={messageSelection.clear}
      />
      {(showScrollDown || newerMessageCount > 0) && (
        <div className="absolute right-4 z-30" style={{ bottom: floatingBottomInset + 16 }}>
        <ScrollToLatestButton
          testId="chat-scroll-bottom"
          onClick={() => {
            showLatest();
            scrollToBottom();
          }}
          title={newerMessageCount > 0 ? `Show ${newerMessageCount} new messages` : "Scroll to latest"}
        />
        </div>
      )}
      </div>
      <div data-composer-height-surface className="ct-chat-bottom-bar shrink-0">
      {isWatcherTab && watcherSpec ? (
        <WatcherReadOnlyBar
          watcher={watcherSpec}
          modelBubble={watcherModelBubble}
        />
      ) : (
      <>
      {chatChannelLoading && setupView.kind === "none" && <TabLoadingIndicator label={`${provider?.displayName ?? tabProvider} loading chat`} />}
      <div ref={setContextSlot} className="ct-ctx-slot" />
      <div className="ct-composer-status w-full">
        <AgentStateIndicator
          tab={tab}
          activity={agentActivity}
          showActivity={setupView.kind === "none" && !chatChannelLoading}
          providerName={provider?.displayName ?? tabProvider}
          model={workingModelBadge}
          reasoningEffort={tab?.reasoning_effort_identity}
          className="ct-composer-lane mx-auto w-full"
          onStop={effectiveChatBackend || !currentPtyId ? undefined : handleStopAgent}
          onCancel={effectiveChatBackend || !currentPtyId ? undefined : handleStopAgent}
          onOpenTerminal={effectiveChatBackend ? undefined : onOpenTerminalFallback}
          onRebind={sessionBindingSurface.showRecoveryAction
            ? () => setBindingRecoveryOpen(true)
            : undefined}
          renderIdentity={tab ? (label) => <TabStatusLineModelPicker tab={tab} label={label} /> : undefined}
        />        {bindingRecoveryOpen && (
          <SessionBindingRecoveryForm
            tabId={tabId}
            suggestedSessionId={sessionBindingSurface.rebindSessionId}
            onClose={() => setBindingRecoveryOpen(false)}
          />
        )}
      </div>
      {deliveryPlansSink}
      {tab?.managed && <ManagedTabBar managed={tab.managed} activity={agentActivity} onStop={handleStopAgent} />}
      {inputStatus && <div className="ct-composer-lane mx-auto w-full">{inputStatus}</div>}
      {uploadStatus}
      {!managedTabPresentation(tab?.managed).locked && <ChatComposer
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
        inlineAttachmentSink={inlineSinkRef}
        contextSlot={contextSlot}
        onWillSendMessage={willSendMessage}
        addOptimistic={addOptimistic}
        updateOptimistic={updateOptimistic}
        observeChatSendStatus={observeChatSendStatus}
        showComposerError={showComposerError}
        onOpenTerminalFallback={effectiveChatBackend ? undefined : onOpenTerminalFallback}
        promptDock={inlinePrompt && (agentActivity.subtype === "permission" && !inlinePrompt.multi_select && !inlinePrompt.sections?.length
          ? <ComposerApprovalDock key={JSON.stringify(inlinePrompt)} prompt={inlinePrompt} tabId={tabId} active={isActive} onRespond={handleRespondPrompt} />
          : <PendingPromptCard pendingPrompt={inlinePrompt} onRespond={(value) => { void handleRespondPrompt(value).catch(() => {}); }} />)}
      />}
      </>
      )}
      </div>
    </div>
    </PluginNameProvider>
    </ChatBlockHostProvider>
    </LocalArtifactScope>
  );
}
