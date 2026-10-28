import { createNoticePublisher } from "../../lib/notices";
import { useLatestCallback } from "../../hooks/useLatestCallback";
import { isInputBlockedTab } from "../../lib/tabInputPolicy";
import { ChatComposerSurface } from "./ChatComposerSurface";
import { useTabLock } from "../billing/useTabLock";
import { ChildTranscriptScope } from "./ChildTranscriptScope";
import { chatLayoutTokens } from "./chatLayoutTokens";
import { RuntimeQuestionCard } from "./RuntimeQuestionCard";
import { heldMessagesForLaunch } from "./chatSetupModel";
import { useComposerPresentation } from "./useComposerPresentation";
import { ComposerIssuesProvider } from "./ComposerIssuesContext";
import { runtimeResponseDeadlinePending, runtimeStatusPresentation } from "../../lib/agentRuntimeState";
import { useAgentRuntimeState } from "./useAgentRuntimeState";
import { promptMessagePart, promptResponseArgs } from "../../lib/promptAnswers";
import { CHAT_ERROR_TOAST_LIFETIME_MS } from "../Notifications/toastLifetime";
import { ThreadRevertConfirmation } from "./ThreadRevertConfirmation";
import { forkableTurnEnds } from "./messageForkModel";
import { AskRow, ChatTranscriptRow, OutboundReportRow, type ChatTranscriptRowContext, type OutboundReportEntry } from "./ChatTranscriptRow";
import { chatWindowRows } from "./chatWindowRowsModel";
import { chatLiveOperationLabel, projectChatTranscriptActivity } from "./chatTranscriptActivity";
import { LocalArtifactScope } from "@codeterm/chat-ui";
import { chatMessageAccessibleName } from "./chatMessageAccessibleName";
import { documentsFromTranscript } from "./chatDocuments";
import { chatDocumentsStore } from "./chatDocumentsStore";
import { chatChangesStore } from "./chatChangesStore";
import { useDeferredValue, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { invoke } from "../../lib/bridge";
import {
  chatDaySeparators,
  chatLocalDay,
  messageRowTime,
  isSessionIntentOnlyMessage,
} from "@codeterm/chat-ui";
import { ChatBlockHostProvider, HistoryGapProvider, PluginNameProvider, extractPlanRefs } from "@codeterm/chat-ui";
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
  selectTabCommunicationNameMap,
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
import type { ApiConfig, Notification, PendingPrompt } from "../../lib/types";
import { ChatComposer } from "./ChatComposer";
import { ManagedTabBar } from "./ManagedTabBar";
import { managedTabPresentation } from "../../lib/managedTab";
import { BusyState } from "../shared/BusyState";
import { PendingPromptCard } from "./PendingPromptCard";
import { AutoConfirmedPromptRows, autoConfirmationsForSession } from "./PermissionAutoConfirm";
import { AgentStateIndicator } from "./AgentStateIndicator";
import { TabStatusLineModelPicker } from "./StatusLineModelPicker";
import { useTabActivity } from "../../lib/useTabActivity";
import { ComposerBackgroundWorkProvider } from "./ComposerBackgroundWorkContext";
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
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { usePlanRefResolver } from "./usePlanRefResolver";
import { chatPlanRefView } from "./chatPlanRef";
import { useChatLayoutInsets } from "./useChatLayoutInsets";
import { ChatHighlightBudgetProvider } from "@codeterm/chat-ui";
import { useMountedChatWindow } from "./useMountedChatWindow";
import { useChatScroll } from "./useChatScroll";
import { resolveTabNavigationTarget } from "./tabNavigation";
import { useMeshPeerNameResolver } from "./useMeshPeerNameResolver";
import { useAgentReportProjection } from "./agentReportProjection";
import { readTranscriptWindow, transcriptWindowKey } from "./transcriptStore";
import { chatBodyView, chatChannelLoadingRow, chatRailsVisible, placeChatRails } from "./chatBodyVisibility";
import { messageTimeMs, normalizeEpochMs } from "./outboundReportPlacement";
import { useChatMessageReveal } from "./useChatMessageReveal";
import { useSessionBindingSurface } from "./useSessionBindingSurface";
import { SessionBindingRecoveryForm } from "./SessionBindingRecoveryForm";
import { BlueprintChatCards } from "./BlueprintChatCards";
import { useTabChatBlockHost } from "./pinnedBlocks";
import { interruptAgentArgs } from "../../lib/agentInterrupt";
import { useTabChoiceHost } from "./useTabChoiceHost";
import { ChatMessageRow } from "./ChatMessageRow";
import { HoldWhileHidden } from "./HoldWhileHidden";
import { awaitsTranscriptEcho, deliveryPresentation, sendForRow } from "./chatDeliveryModel";
import { beginChatSendRetry, failChatSendRequest, useChatSendSnapshot } from "./chatSendStore";
import { deleteTrackedChatMessage, sendTrackedChatMessage } from "./chatSendStatus";
import { prepareChatDelivery } from "./chatDeliveryPreparation";
import { useDsTasksExpander } from "./dsTasks/useDsTasksExpander";
import { MessageSelectionBar } from "./MessageSelectionBar";
import { useChatMessageSelection } from "./useChatMessageSelection";
import { streamingMessageId } from "./streamingMessage";
import { requestChatMessageReveal } from "../../lib/chatMessageReveal";

interface Props {
  inputStatus?: ReactNode;
  tabId: string;
  api?: ApiConfig;
  onError?: (msg: string) => void;
  onOpenTerminalFallback?: () => void;
  uiFontScale?: number;
  isVisible?: boolean;
  sessionActive?: boolean;
  /** Tab is backed by a plugin chatBackend (no PTY). Sends route through the
   *  tab send seam, and PTY-only affordances (terminal fallback, screen poll)
   *  are suppressed. */
  chatBackend?: boolean;
}

const DISABLED_PLAN_API: ApiConfig = { enabled: false, host: "127.0.0.1", port: 0, token: null, cors: false };

const CHAT_TIME_ZONE = Intl.DateTimeFormat().resolvedOptions().timeZone;
const NO_RAIL_INDICES: readonly number[] = [];
const NO_REPORTS: readonly OutboundReportEntry[] = [];
const NO_ASKS: readonly Notification[] = [];

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

export function TabSessionChat(props: Props) {
  return <ComposerIssuesProvider key={props.tabId}><TabSessionChatBody {...props} /></ComposerIssuesProvider>;
}

function TabSessionChatBody({
  inputStatus,
  tabId,
  api,
  onError,
  onOpenTerminalFallback,
  uiFontScale,
  isVisible = true,
  sessionActive,
  chatBackend = false,
}: Props) {
  if (isCodeTermFrontendTestMode()) {
    const counts = (window.__renderCounts ??= {});
    counts[tabId] = (counts[tabId] ?? 0) + 1;
    window.__chatProjection ??= chatProjectionMonitor;
  }

  const touchMode = useTouchOptimizedUi();
  const { sends: deliverySends } = useChatSendSnapshot(tabId);
  const { expand: expandDeliveryTasks, plansSink: deliveryPlansSink } = useDsTasksExpander(tabId);
  const tabBlockHost = useTabChatBlockHost(tabId);
  const lastStopAtRef = useRef(0);
  const chatRootRef = useRef<HTMLDivElement>(null);
  const floatingBottomInset = useChatLayoutInsets(chatRootRef);
  const chatFollowTailRef = useRef(true);

  const { setFocusedTab, selectTabByTab, upsertToast, answerNotification } = useMultiplexer();
  const openPlanTarget = usePlanTargetNavigation();
  const tabSelector = useLatestCallback((s: Parameters<typeof selectTab>[0]) => selectTab(s, tabId), [tabId]);
  const tab = useMuxSelector(tabSelector, tabFieldsEqual);
  const tabLock = useTabLock(tab);
  const readOnly = isInputBlockedTab(tab, tabLock);
  const workspaceSelector = useLatestCallback((s: Parameters<typeof selectTab>[0]) => s.workspaces.find((workspace) => workspace.tabs.includes(tabId))?.id ?? s.tabs[tabId]?.workspace_id ?? null, [tabId]);
  const workspaceId = useMuxSelector(workspaceSelector);
  const tabTitleMap = useMuxSelector(selectTabTitleMap, tabTitleMapEqual);
  const tabCommunicationNames = useMuxSelector(selectTabCommunicationNameMap, tabTitleMapEqual);
  const planNav = useMuxSelector(selectPlanNavContext, planNavContextEqual);
  const tabAgentTypes = useMuxSelector(selectTabAgentTypes, tabAgentTypesEqual);
  const askSelector = useLatestCallback((s: Parameters<typeof selectTabAsks>[0]) => selectTabAsks(s, tabId), [tabId]);
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
  const runtime = useAgentRuntimeState(tabId, tab?.session_binding_nonce ?? null, tab?.agent_runtime_state, tab?.agent_runtime_questions);
  const currentPtyId = tab?.pty_id ?? null;
  const agentActivity = useTabActivity(tabId);
  const sessionBindingSurface = useSessionBindingSurface(tabId, agentSessionId);
  const chatBacking = tab?.backing?.kind === "chat_backend" ? tab.backing : null;
  const effectiveChatBackend = chatBackend || chatBacking !== null;
  const watcherSpec = tab?.watcher ?? null;
  const isWatcherTab = watcherSpec != null;
  const pendingPrompt: PendingPrompt | null = tab?.pending_prompt ?? null;
  const descriptor = getAgentDescriptor(tabProvider);
  const promptAnswers = descriptor?.supports_prompt_answers === true;
  const structuredPrompts = descriptor?.supports_structured_prompts === true;
  const autoConfirmations = autoConfirmationsForSession(tab?.permission_auto_confirmations, agentSessionId);
  const isActive = isVisible;
  const resolvePeerName = useMeshPeerNameResolver(isActive);
  const resolvePluginName = usePluginDisplayNameResolver();

  const showComposerError = useLatestCallback((message: string) => {
    createNoticePublisher({ upsertToast })({ kind: "toast", tabId: tabId,
      workspaceId: null,
      title: isMicrophonePermissionError(message)
        ? "Microphone unavailable"
        : "Chat error",
      detail: isMicrophonePermissionError(message)
        ? `${message} Open microphone permissions in Settings if needed.`
        : message,
      severity: "Error",
      lifetimeMs: CHAT_ERROR_TOAST_LIFETIME_MS });
  }, [upsertToast, tabId]);

  const {
    loading,
    recovering,
    error,
    retry: retryAttachment,
    attachmentState,
    snapshotReady,
    transcriptUnavailable,
    allMessages,
    enrichedMessages,
    hiddenMessageCount,
    olderMessageLabel,
    newerMessageCount,
    canBackfillOlder,
    backfillLoading,
    backfillNotice,
    loadOlder,
    loadHistoryGap,
    historyGapLabel,
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
    active: sessionActive ?? isActive,
    retainAttachment: true,
    diagnosticSink: chatProjectionMonitor.sink,
    allowUnboundAuthority: effectiveChatBackend,
    followTailRef: chatFollowTailRef,
    history: runtime.state?.history,
    live: isActive || awaitsTranscriptEcho(deliverySends),
  });
  const retryChatDelivery = useLatestCallback(async (id: string) => {
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
  }, [tabId, expandDeliveryTasks, observeChatSendStatus]);
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
  const forkableMessages = useMemo(() => forkableTurnEnds(
    allMessages,
    descriptor?.agent_runtime?.threadHistory?.actions.includes("fork") === true,
    newerMessageCount > 0,
  ), [allMessages, descriptor, newerMessageCount]);
  const heldAuthority = readTranscriptWindow(transcriptWindowKey(tabId, agentSessionId)).transcript.authority;
  const hasHeldWindow = heldAuthority != null
    && heldAuthority.sourceSessionId === agentSessionId
    && heldAuthority.sessionBindingNonce === (tab?.session_binding_nonce ?? null);
  // 'Loading chat' gates on the CHAT channel only (transcript authority + chat
  // reachability); remote PTY/output channel state never gates it.
  const runtimeDeadlinePending = runtimeResponseDeadlinePending(runtime.state);
  const runtimeUnavailable = runtimeStatusPresentation(runtime.state) !== null && !runtimeDeadlinePending;
  const { runtimeBadge, sessionIntent } = useComposerPresentation({ tabId, nonce: tab?.session_binding_nonce ?? null,
    state: runtime.state, retry: runtime.retry, startupOwned: !snapshotReady || bootSetupView.kind === "starting",
    iconSvg: provider?.iconSvg, messages: allMessages });
  const runtimeReplacement = runtimeUnavailable ? runtimeBadge : undefined;
  const runtimeFailed = runtime.state?.state === "unloaded" || runtime.state?.state === "degraded";
  const remoteChatReconnecting = tab?.remote_channels?.chat === "reconnecting";
  const chatChannelLoading = !tabLock.locked && attachmentState !== "failed" && !runtimeFailed && (loading || recovering || (!hasTranscript && remoteChatReconnecting));
  const chatLoading = chatChannelLoading;
  const setupView = runtimeUnavailable && snapshotReady && !tabLock.locked && attachmentState !== "failed" && bootSetupView.kind !== "launch_failed" ? { kind: "none" as const } : chatBodyView({
    hasTranscript,
    hasHeldWindow,
    snapshotReady,
    chatLoading,
    unavailable: transcriptUnavailable,
    bootView: bootSetupView,
    attachmentError: attachmentState === "failed" ? error : null,
    locked: tabLock.locked,
  });

  const renderedEnrichedMessages = useMemo(
    () => setupView.kind !== "none" ? [] : chatWindowRows(enrichedMessages.filter(({ msg }) => !isSessionIntentOnlyMessage(msg))),
    [enrichedMessages, setupView.kind],
  );
  const transcriptActivityAvailable = setupView.kind === "none" && !chatLoading;
  const chatTranscriptActivity = useMemo(
    () => projectChatTranscriptActivity(
      renderedEnrichedMessages,
      transcriptActivityAvailable ? agentActivity.kind : "idle",
      descriptor,
      tab?.backing,
      effectiveChatBackend,
    ),
    [agentActivity.kind, descriptor, effectiveChatBackend, renderedEnrichedMessages, tab?.backing, transcriptActivityAvailable],
  );
  const sharedScrollRef = useRef<HTMLDivElement | null>(null);
  const chatUserScrollingRef = useRef(false);
  const mountedWindow = useMountedChatWindow({ rows: renderedEnrichedMessages, scrollRef: sharedScrollRef,
    active: isActive, followTailRef: chatFollowTailRef, sessionKey: `${tabId}:${agentSessionId ?? ""}`, userScrollingRef: chatUserScrollingRef });
  const mountedContentVersion = useMemo(() => ({ enrichedMessages, start: mountedWindow.range.start,
    end: mountedWindow.range.end, revision: mountedWindow.revision }),
    [enrichedMessages, mountedWindow.range.start, mountedWindow.range.end, mountedWindow.revision]);
  const {
    scrollRef,
    showScrollDown,
    gestureOwned,
    scrollToBottom,
    followSend: rawFollowSentMessage,
    prepareForHistoryPrepend,
    handleScroll,
    handleContentLayout,
    revealMessage,
  } = useChatScroll({
    scrollRef: sharedScrollRef,
    onViewportChange: mountedWindow.syncViewport,
    userScrollingRef: chatUserScrollingRef,
    foldLayoutDrift: mountedWindow.foldDrift,
    sessionKey: `${tabId}:${agentSessionId ?? ""}`,
    active: isActive,
    loading,
    messageCount: enrichedMessages.length,
    contentVersion: mountedContentVersion,
    prependVersion: enrichedMessages[0]?.msg.id ?? null,
    followTailRef: chatFollowTailRef,
    onModeChange: (mode) => setHistoryReading(mode === "detached"),
  });
  useChatMessageReveal({
    tabId,
    ready: isActive && !chatChannelLoading,
    contentVersion: mountedContentVersion,
    revealMessage,
    prepareMessage: (id) => mountedWindow.prepareMessage(id) || prepareMessage(id),
  });

  const followSentMessage = useLatestCallback(() => {
    showLatest();
    rawFollowSentMessage();
  }, [showLatest, rawFollowSentMessage]);
  const choiceHost = useTabChoiceHost(tabId, { messages: allMessages, addOptimistic, followSentMessage, observeChatSendStatus });
  const chatBlockHost = useMemo(() => ({ ...tabBlockHost, choice: choiceHost }), [tabBlockHost, choiceHost]);

  const willSendMessage = useLatestCallback(() => {
    followSentMessage();
  }, [followSentMessage]);

  const [answeredInteractiveIds, setAnsweredInteractiveIds] = useState<Set<string>>(new Set());
  const [bindingRecoveryOpen, setBindingRecoveryOpen] = useState(false);
  const [pendingThreadRevertTurn, setPendingThreadRevertTurn] = useState<string | null>(null);
  const [contextSlot, setContextSlot] = useState<HTMLDivElement | null>(null);
  const [answeredPromptQuestion, setAnsweredPromptQuestion] = useState<string | null>(null);
  const runThreadHistory = useLatestCallback(async (action: "fork" | "revert", turnId: string, confirmed = false) => {
    const threadId = tab?.agent_session_id;
    const bindingNonce = tab?.session_binding_nonce;
    const epoch = runtime.state?.epoch;
    if (!threadId || !bindingNonce || !epoch) {
      showComposerError("Runtime thread binding is unavailable");
      return;
    }
    try {
      await invoke("agent_runtime_thread_history", {
        tabId,
        request: { action, turnId, confirmed, binding: { threadId, bindingNonce, epoch } },
      });
    } catch (error) {
      showComposerError(error instanceof Error ? error.message : String(error));
    }
  }, [runtime.state?.epoch, showComposerError, tab?.agent_session_id, tab?.session_binding_nonce, tabId]);
  useEffect(() => {
    if (!sessionBindingSurface.showRecoveryAction) setBindingRecoveryOpen(false);
  }, [sessionBindingSurface.showRecoveryAction]);
  const promptPart = promptMessagePart(descriptor, pendingPrompt, answeredPromptQuestion);

  useEffect(() => {
    if (!pendingPrompt) setAnsweredPromptQuestion(null);
  }, [pendingPrompt]);

  const promptResponseInFlight = useRef(false);
  const handleRespondPrompt = useLatestCallback(
    async (value: string) => {
      if (promptResponseInFlight.current) return;
      promptResponseInFlight.current = true;
      if (pendingPrompt && !structuredPrompts) setAnsweredPromptQuestion(pendingPrompt.question);
      try {
        await invoke("mux_respond_prompt", promptResponseArgs(tabId, value, pendingPrompt));
      } catch (error) {
        setAnsweredPromptQuestion(null);
        showComposerError(error instanceof Error ? error.message : String(error));
        throw error;
      } finally {
        promptResponseInFlight.current = false;
      }
    },
    [pendingPrompt, structuredPrompts, tabId, showComposerError],
  );
  // input path the composer uses; jump surfaces the live terminal output.
  const handleRerunShellCommand = useLatestCallback((command: string) => {
    if (!currentPtyId) {
      showComposerError("Tab has no PTY");
      return;
    }
    void invoke("write_pty_string", { ptyId: currentPtyId, data: command + "\r", raw: false }).catch(
      (e) => showComposerError(e instanceof Error ? e.message : String(e)),
    );
  }, [currentPtyId, showComposerError, tabId, tab?.managed]);

  const handleJumpToShellCommand = useLatestCallback(() => {
    onOpenTerminalFallback?.();
  }, [onOpenTerminalFallback]);

  const focusComposer = useLatestCallback(() => {
    chatRootRef.current?.querySelector<HTMLTextAreaElement>("textarea")?.focus();
  }, []);

  const interruptTarget = useMemo(() => ({ id: tabId, pty_id: currentPtyId, managed: tab?.managed, backing: tab?.backing }),
    [tabId, currentPtyId, tab?.managed, tab?.backing]);
  const canInterrupt = interruptAgentArgs(interruptTarget) !== null;
  const handleStopAgent = useLatestCallback(() => {
    const args = interruptAgentArgs(interruptTarget);
    if (!args) {
      showComposerError("Tab has no interrupt route");
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
  }, [interruptTarget, showComposerError]);

  const openJobs = useLatestCallback(() => {
    window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:jobs" } }));
  }, []);

  const handleStopReminder = useLatestCallback(async (id: string) => {
    try {
      await invoke("delayed_send_delete", { id });
    } catch (error) {
      showComposerError(error instanceof Error ? error.message : String(error));
    }
  }, [showComposerError]);

  const handleAnswerInteractive = useLatestCallback<InteractiveAnswerHandler>(
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
    enabled: !loading && !error && !readOnly,
    onError: showComposerError,
  });
  const pasteIntoComposer = useLatestCallback(handlePasteEvent, [tabId]);

  // Names change far more rarely than the transcript that carries them; keep the
  // map (and resolveTabName below) stable while streaming only appends content.
  const communicationTabNamesRef = useRef<{ key: string; value: Map<string, string> }>({
    key: "",
    value: new Map(),
  });
  const communicationTabNames = useMemo(() => {
    const names = new Map(tabCommunicationNames);
    for (const { origin } of enrichedMessages) {
      if (origin?.kind !== "tab") continue;
      const sender = origin.title.split("@")[0]?.trim();
      if (sender && !names.has(origin.tabIdShort.slice(0, 8))) names.set(origin.tabIdShort.slice(0, 8), sender);
    }
    const key = JSON.stringify([...names]);
    const prev = communicationTabNamesRef.current;
    if (key === prev.key) return prev.value;
    communicationTabNamesRef.current = { key, value: names };
    return names;
  }, [enrichedMessages, tabCommunicationNames]);

  const resolveTabName = useLatestCallback(
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
    return entry ? <OutboundReportRow key={entry.descriptor.reportId} entry={entry} context={rowContext} /> : null;
  };
  const renderAsk = (index: number) => {
    const ask = asks[index];
    return ask ? <AskRow key={ask.id} ask={ask} context={rowContext} /> : null;
  };

  const localToday = chatLocalDay(Date.now(), CHAT_TIME_ZONE);
  const daySeparators = useMemo(() => chatDaySeparators(renderedEnrichedMessages.map(({ msg, item, origin }) => messageRowTime(msg, item, origin)?.timestamp ?? null), {
    now: Date.now(), timeZone: CHAT_TIME_ZONE,
    labels: { today: "Today", yesterday: "Yesterday" },
  }), [renderedEnrichedMessages, localToday]);
  const streamingId = streamingMessageId(renderedEnrichedMessages, agentActivity.active);
  const messageSelection = useChatMessageSelection({ tabId, rows: renderedEnrichedMessages, agent: tab?.title || provider?.displayName || "Agent", surfaceRef: scrollRef, streamingMessageId: streamingId, prepareMessage: mountedWindow.prepareMessage, contentVersion: mountedContentVersion });
  const jumpToQuoted = useLatestCallback((messageId: string) => { requestChatMessageReveal(tabId, messageId); }, [tabId]);

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
  const resolvePlanRef = useLatestCallback(
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

  const onOpenPlan = useLatestCallback(
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

  const resolveFocusTab = useLatestCallback((ref: string) => {
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

  const deleteSend = useLatestCallback((sendId: string) => {
    void deleteTrackedChatMessage(tabId, sendId).catch((error: unknown) => showComposerError(error instanceof Error ? error.message : String(error)));
  }, [tabId, showComposerError]);
  const forkFrom = useLatestCallback((turnId: string) => { void runThreadHistory("fork", turnId); }, [runThreadHistory]);
  const revertTo = useLatestCallback((turnId: string) => setPendingThreadRevertTurn(turnId), []);
  const canRevertTurns = descriptor?.agent_runtime?.threadHistory?.actions.includes("revert") === true;
  const rowContext = useMemo<ChatTranscriptRowContext>(() => ({
    tabId,
    promptAnswers,
    uiFontScale,
    answeredInteractiveIds,
    handlers: messageSelection.rowHandlers,
    onContentLayout: handleContentLayout,
    resolveTabName,
    resolvePeerName,
    onOpenPlan,
    resolvePlanRef,
    onAnswerInteractive: handleAnswerInteractive,
    onRerunShellCommand: handleRerunShellCommand,
    onStopReminder: handleStopReminder,
    onOpenJobs: openJobs,
    onJumpToShellCommand: onOpenTerminalFallback ? handleJumpToShellCommand : undefined,
    onFocusTab: resolveFocusTab,
    onJumpToMessage: jumpToQuoted,
    onAnswerAsk: answerNotification,
    deleteSend,
    retrySend: retryChatDelivery,
    forkFrom,
    revertTo,
  }), [tabId, promptAnswers, uiFontScale, answeredInteractiveIds, messageSelection.rowHandlers, handleContentLayout, resolveTabName,
    resolvePeerName, onOpenPlan, resolvePlanRef, handleAnswerInteractive, handleRerunShellCommand, handleStopReminder, openJobs,
    onOpenTerminalFallback, handleJumpToShellCommand, resolveFocusTab, jumpToQuoted, answerNotification, deleteSend, retryChatDelivery,
    forkFrom, revertTo]);

  const showEmptyState = setupView.kind === "none"
    && !chatLoading
    && renderedEnrichedMessages.length === 0
    && outboundReports.length === 0
    && autoConfirmations.length === 0
    && !promptPart;

  return (
    <ChatHighlightBudgetProvider>
    <ChildTranscriptScope tabId={tabId} authority={runtime.state?.state === "loaded" && tab?.agent_session_id && tab.session_binding_nonce
      ? {rootThreadId:tab.agent_session_id,bindingNonce:tab.session_binding_nonce,epoch:runtime.state.epoch} : null}>
    <LocalArtifactScope tabId={tabId}>
    <ChatBlockHostProvider value={chatBlockHost}>
    <PluginNameProvider resolve={resolvePluginName}>
    <HistoryGapProvider label={historyGapLabel} onLoad={async message => {
      prepareForHistoryPrepend();
      await loadHistoryGap(message);
    }}>
    <div
      ref={chatRootRef}
      className="ct-chat-shell w-full h-full flex flex-col relative"
      data-testid="tab-session-chat"
      data-tab-id={tabId}
      data-snapshot-ready={snapshotReady}
      role="region"
      aria-label={`Tab chat: ${tab?.title ?? tabId}`}
      aria-busy={setupView.kind !== "failed" && setupView.kind !== "launch_failed" && (!snapshotReady || chatChannelLoading)}
      style={{
        ...chatLayoutTokens().variables,
        background: "var(--ct-bg, #0d1117)",
        color: "var(--ct-fg, #c9d1d9)",
      }}
      {...(readOnly ? {} : webDropHandlers)}
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
        {railsVisible && (hiddenMessageCount > 0 || canBackfillOlder) && (
          <div className="flex justify-center pb-2 pt-1">
            <button
              type="button"
              data-testid="chat-load-older"
              aria-label={olderMessageLabel}
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
              {backfillLoading
                ? <BusyState inline hideLabel label="Searching older messages" phase="work" />
                : olderMessageLabel}
            </button>
          </div>
        )}
        {backfillNotice === "stalled_cursor"
          ? <InlineErrorNotice error="Older history could not be read." operation="loading older messages" />
          : backfillNotice && <p role="status" className="text-center text-xs text-[var(--ct-muted-fg)]">
            {backfillNotice === "limit" ? "History scan limit reached. Load previous to continue." : "No earlier messages."}
          </p>}
        {isActive && (reportPlacement.afterIndex.get(-1) ?? []).map(renderOutboundReport)}
        {isActive && (askPlacement.afterIndex.get(-1) ?? []).map(renderAsk)}
        {isActive && railsVisible && <BlueprintChatCards tabId={tabId} workspaceId={workspaceId} />}
        <div ref={mountedWindow.topSpacerRef} aria-hidden="true" style={{ height: mountedWindow.topSpacer }} />
        <div ref={mountedWindow.rowsRef}>
        <HoldWhileHidden visible={isActive}>
        {renderedEnrichedMessages.slice(mountedWindow.range.start, mountedWindow.range.end).map((entry, mountedIndex) => {
          const { msg } = entry;
          const messageIndex = mountedWindow.range.start + mountedIndex;
          const following = reportPlacement.afterIndex.get(messageIndex) ?? NO_RAIL_INDICES;
          const followingAsks = askPlacement.afterIndex.get(messageIndex) ?? NO_RAIL_INDICES;
          const nextId = renderedEnrichedMessages[messageIndex + 1]?.msg.id;
          const deliverySend = sendForRow(deliverySends, msg);
          return (
            <ChatTranscriptRow
              key={msg.id}
              entry={entry}
              daySeparator={daySeparators.get(messageIndex)}
              originProps={originPropsById.get(msg.id)}
              reports={following.length ? following.map((index) => outboundReports[index]).filter(Boolean) : NO_REPORTS}
              asks={followingAsks.length ? followingAsks.map((index) => asks[index]).filter(Boolean) : NO_ASKS}
              liveActivityLabel={msg.id === chatTranscriptActivity.liveToolId ? chatLiveOperationLabel(msg) : undefined}
              deliverySend={deliverySend}
              deliveryView={deliverySend ? deliveryPresentation(deliverySend, agentActivity.kind) : undefined}
              sourceOrdinal={mountedWindow.identities[messageIndex].ordinal}
              ariaLabel={chatMessageAccessibleName(msg.type, msg.content)}
              selected={messageSelection.selected.has(msg.id)}
              joinNext={nextId !== undefined && following.length === 0 && messageSelection.selected.has(nextId)}
              selecting={messageSelection.active}
              cursor={messageSelection.cursor ? messageSelection.cursor === msg.id : messageIndex === renderedEnrichedMessages.length - 1}
              streaming={msg.id === streamingId}
              latestFailedToolCall={msg.id === latestFailedToolCallId}
              forkable={forkableMessages.has(msg.id)}
              revertible={canRevertTurns}
              context={rowContext}
            />
          );
        })}
        </HoldWhileHidden>
        </div>
        <div aria-hidden="true" style={{ height: mountedWindow.range.bottom }} />
        {runtime.questions.map((item) => <RuntimeQuestionCard key={JSON.stringify([tab?.session_binding_nonce, item.threadId, item.itemId])} tabId={tabId} item={item} onReceipt={observeChatSendStatus} />)}
        {promptPart && <ChatMessageRow
          msg={{ id: promptPart.id, type: "assistant" }}
          ariaLabel={chatMessageAccessibleName("assistant", promptPart.prompt.question)}
          selected={false}
          joinNext={false}
          selecting={false}
          cursor={false}
          handlers={messageSelection.rowHandlers}
        >
          <div data-message-part={promptPart.id}>
            <PendingPromptCard key={promptPart.id} pendingPrompt={promptPart.prompt} onRespond={(value) => { void handleRespondPrompt(value).catch(() => {}); }} />
          </div>
        </ChatMessageRow>}
        {isActive && reportPlacement.trailing.map(renderOutboundReport)}
        {isActive && askPlacement.trailing.map(renderAsk)}
        <AutoConfirmedPromptRows records={autoConfirmations} />
        </div>
      </div>
        <ChatSetupState
          visible={isActive}
          waitingMessages={heldMessagesForLaunch(deliverySends)}
          workspace={planNav.workspaces.find(workspace => workspace.id === workspaceId)}
          tab={tab}
          view={setupView}
          providerName={provider?.displayName ?? "the agent"}
          providerIconSvg={provider?.iconSvg}
          providerIconColor={provider?.iconColor}
          providerIconHint={provider?.iconHint}
          onEscape={focusComposer}
          onRetry={retryAttachment}
          lock={tabLock}
        />
      <MessageSelectionBar
        count={messageSelection.count}
        partial={messageSelection.partial}
        onCopy={messageSelection.copySelection}
        onQuote={messageSelection.quoteSelection}
        onAsk={messageSelection.askSelection}
        onClear={messageSelection.clear}
      />
      {(showScrollDown || newerMessageCount > 0) && (
        <div className="absolute right-4 z-30" style={{ bottom: floatingBottomInset + 16, pointerEvents: "none" }}>
        <ScrollToLatestButton
          testId="chat-scroll-bottom"
          gestureOwned={gestureOwned}
          userScrollingRef={chatUserScrollingRef}
          onClick={() => {
            showLatest();
            scrollToBottom();
          }}
          title={newerMessageCount > 0 ? `Show ${newerMessageCount} new messages` : "Scroll to latest"}
        />
        </div>
      )}
      </div>
      <ComposerBackgroundWorkProvider tabId={tabId} messages={allMessages}>
      <ChatComposerSurface tab={tab} lock={tabLock} view={setupView} providerId={tabProvider ?? undefined} tabId={tabId}>
      {isWatcherTab && watcherSpec ? (
        runtimeReplacement ?? <WatcherReadOnlyBar
          watcher={watcherSpec}
          modelBubble={watcherModelBubble}
        />
      ) : (
      <>
      <div ref={setContextSlot} className="ct-ctx-slot" />
      <div className="ct-composer-status w-full">
        <AgentStateIndicator
          tab={tab}
          activity={agentActivity}
          activeTool={allMessages.find((message) => message.type === "tool_call" && message.toolStatus === "running")}
          runtimeState={runtime.state}
          showActivity={setupView.kind === "none" && !chatLoading}
          channelLoadingLabel={chatChannelLoadingRow(chatLoading, setupView, snapshotReady, hasHeldWindow) === "model"
            ? `${provider?.displayName ?? tabProvider} loading chat` : undefined}
          providerName={provider?.displayName ?? tabProvider}
          model={workingModelBadge}
          reasoningEffort={tab?.reasoning_effort_identity}
          className="ct-composer-lane mx-auto w-full"
          onStop={canInterrupt ? handleStopAgent : undefined}
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
      {pendingThreadRevertTurn && <ThreadRevertConfirmation
        turnId={pendingThreadRevertTurn}
        onCancel={() => setPendingThreadRevertTurn(null)}
        onConfirm={(turnId) => {
          setPendingThreadRevertTurn(null);
          void runThreadHistory("revert", turnId, true);
        }}
      />}
        {managedTabPresentation(tab?.managed).locked ? runtimeReplacement : <ChatComposer
          tabId={tabId}
          tabProvider={tabProvider}
          currentPtyId={currentPtyId}
          chatBackend={effectiveChatBackend}
          isActive={isActive}
          promptDock={runtimeBadge}
          pendingPrompt={pendingPrompt}
          attachments={attachments}
          addAttachments={addAttachments}
          removeAttachment={removeAttachment}
          clearAttachments={clearAttachments}
          handlePasteEvent={pasteIntoComposer}
          inlineAttachmentSink={inlineSinkRef}
          pluginError={!runtimeUnavailable && effectiveChatBackend ? error : null}
          sessionIntent={sessionIntent}
          contextSlot={contextSlot}
          onWillSendMessage={willSendMessage}
          addOptimistic={addOptimistic}
          updateOptimistic={updateOptimistic}
          observeChatSendStatus={observeChatSendStatus}
          showComposerError={showComposerError}
          onOpenTerminalFallback={effectiveChatBackend ? undefined : onOpenTerminalFallback}
        />}
      </>
      )}
      </ChatComposerSurface>
      </ComposerBackgroundWorkProvider>
    </div>
    </HistoryGapProvider>
    </PluginNameProvider>
    </ChatBlockHostProvider>
    </LocalArtifactScope>
    </ChildTranscriptScope>
    </ChatHighlightBudgetProvider>
  );
}
