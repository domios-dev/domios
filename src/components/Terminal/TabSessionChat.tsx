import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import { invoke } from "../../lib/bridge";
import { MessageBubble } from "@codeterm/chat-ui";
import { extractPlanRefs } from "@codeterm/chat-ui";
import type { InteractiveAnswerHandler } from "@codeterm/chat-ui";
import { AgentIcon } from "../AgentIcon";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import {
  tabAgentTypesEqual,
  tabFieldsEqual,
  tabTitleMapEqual,
  planNavContextEqual,
  selectTab,
  selectTabAgentTypes,
  selectTabTitleMap,
  selectPlanNavContext,
} from "../../store/multiplexer/selectors";
import { useTabProvider, useShellSessionModel } from "../../lib/useTabProvider";
import { formatModelBadge } from "../../lib/formatModelBadge";
import { usePlanIndexOptional } from "../Canvas/PlansProvider";
import { openPlanDeepLink } from "../Canvas/workspacePlanEntry";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import type { ApiConfig, PendingPrompt } from "../../lib/types";
import { ChatComposer } from "./ChatComposer";
import { PendingPromptCard } from "./PendingPromptCard";
import { AgentStateIndicator } from "./AgentStateIndicator";
import type { AgentState } from "./AgentStateIndicator";
import { useAttachments } from "./useAttachments";
import { useChatSession, isCodeTermFrontendTestMode, paintTailSignature } from "./useChatSession";
import { chatProjectionMonitor, scheduleAfterPaint } from "./chatProjectionDiagnostics";
import { ChatModelBubble } from "../Extensions/ChatModelBubble";
import { ChatPresetBubble } from "../Extensions/ChatPresetBubble";
import { WatcherReadOnlyBar } from "./WatcherReadOnlyBar";
import { ChatSetupState, ChatSetupStatusLine, chatSetupView } from "./ChatSetupState";
import { usePlanRefResolver } from "./usePlanRefResolver";
import { reattachTabChat } from "./chatReattach";
import { chatPlanRefView } from "./chatPlanRef";
import { useChatScroll } from "./useChatScroll";
import { resolveTabNavigationTarget } from "./tabNavigation";
import { waitingQuestionTracker } from "./waitingQuestionTracker";

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
  const [longIdle, setLongIdle] = useState(false);

  const { setFocusedTab, selectTabByTab, addNotification, setActiveWorkspace } = useMultiplexer();
  const tabSelector = useCallback((s: Parameters<typeof selectTab>[0]) => selectTab(s, tabId), [tabId]);
  const tab = useMuxSelector(tabSelector, tabFieldsEqual);
  const teamNameSelector = useCallback((state: Parameters<typeof selectTab>[0]) => {
    const teamId = (state.tabs[tabId] as (typeof state.tabs)[string] & { team_id?: string | null } | undefined)?.team_id;
    return teamId ? state.teams.find((team) => team.id === teamId)?.name ?? null : null;
  }, [tabId]);
  const teamName = useMuxSelector(teamNameSelector);
  const projectNameSelector = useCallback((state: Parameters<typeof selectTab>[0]) => {
    const workspace = state.workspaces.find((candidate) => candidate.tabs.includes(tabId));
    return workspace?.name ?? null;
  }, [tabId]);
  const projectName = useMuxSelector(projectNameSelector);
  const tabTitleMap = useMuxSelector(selectTabTitleMap, tabTitleMapEqual);
  const planNav = useMuxSelector(selectPlanNavContext, planNavContextEqual);
  const tabAgentTypes = useMuxSelector(selectTabAgentTypes, tabAgentTypesEqual);
  const { index: planIndex } = usePlanIndexOptional();
  const tabProvider = tab?.agent_type ?? null;
  const provider = useTabProvider(tab);
  const shellSessionModel = useShellSessionModel(tab);
  const tabModel = tab?.agent_session_model ?? shellSessionModel;
  const workingModelBadge = tabModel ? formatModelBadge(tabModel) : null;
  const agentSessionId = tab?.agent_session_id ?? null;
  const currentPtyId = tab?.pty_id ?? null;
  const backendState = tab?.agent_state ?? null;
  const [waitingGeneration, setWaitingGeneration] = useState<number | null>(null);
  const chatBacking = tab?.backing?.kind === "chat_backend" ? tab.backing : null;
  const effectiveChatBackend = chatBackend || chatBacking !== null;
  const watcherSpec = tab?.watcher ?? null;
  const isWatcherTab = watcherSpec != null;
  const pendingPrompt: PendingPrompt | null = tab?.pending_prompt ?? null;
  const isActive = isVisible;

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
      level: "Error",
    });
  }, [addNotification, tabId]);

  const {
    loading,
    recovering,
    error,
    attachmentState,
    retry,
    enrichedMessages,
    hiddenMessageCount,
    hiddenNewerCount,
    canBackfillOlder,
    backfillLoading,
    loadOlder,
    showLatest,
    addOptimistic,
    updateOptimistic,
  } = useChatSession({
    tabId,
    agentSessionId,
    sessionBindingNonce: tab?.session_binding_nonce,
    onError,
    active: isActive,
    diagnosticSink: chatProjectionMonitor.sink,
    allowUnboundAuthority: effectiveChatBackend,
  });
  const bootSetupView = chatSetupView(tab, attachmentState === "attached");

  const {
    scrollRef,
    contentRef: scrollContentRef,
    showScrollDown,
    scrollToBottom,
    followSend: rawFollowSentMessage,
    handleScroll,
  } = useChatScroll({
    sessionKey: `${tabId}:${agentSessionId ?? ""}`,
    active: isActive,
    loading,
    messageCount: enrichedMessages.length,
  });

  // Sending or jumping to latest must leave history-browsing mode first, or the
  // bounded window would keep the new tail out of the DOM.
  const followSentMessage = useCallback(() => {
    showLatest();
    rawFollowSentMessage();
  }, [showLatest, rawFollowSentMessage]);

  const [answeredInteractiveIds, setAnsweredInteractiveIds] = useState<Set<string>>(new Set());
  // clears tab.pending_prompt shortly after on the next state poll.
  const [answeredPromptQuestion, setAnsweredPromptQuestion] = useState<string | null>(null);
  const activePrompt = pendingPrompt && pendingPrompt.question !== answeredPromptQuestion ? pendingPrompt : null;
  const inlinePrompt = effectiveChatBackend ? activePrompt : null;

  useEffect(() => {
    if (!pendingPrompt) setAnsweredPromptQuestion(null);
  }, [pendingPrompt]);

  const handleRespondPrompt = useCallback(
    (value: string) => {
      if (pendingPrompt) setAnsweredPromptQuestion(pendingPrompt.question);
      void invoke("mux_respond_prompt", { tabId, value })
        .then(() => {
          if (!pendingPrompt || !agentSessionId) return;
          waitingQuestionTracker.resolve(tabId, agentSessionId, pendingPrompt.question);
          void invoke("notify_agent_input_request", {
            tabId,
            sessionId: agentSessionId,
            promptIdentity: pendingPrompt.question,
            resolved: true,
          }).catch((error) => {
            console.warn("Failed to resolve waiting-for-input notification:", error);
          });
        })
        .catch((e) => showComposerError(e instanceof Error ? e.message : String(e)));
    },
    [agentSessionId, pendingPrompt, tabId, showComposerError],
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

  const handleLaunchRetry = useCallback(() => {
    void invoke("restart_agent_tab", { tabId }).catch((error) =>
      showComposerError(error instanceof Error ? error.message : String(error)),
    );
  }, [tabId, showComposerError]);

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

  const resolveTabName = useCallback(
    (tabRef: string): string | undefined => tabTitleMap.get(tabRef.slice(0, 8)),
    [tabTitleMap],
  );

  const renderedEnrichedMessages = enrichedMessages;
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
      openPlanDeepLink(planId, taskId, {
        index: planIndex,
        workspaces: planNav.workspaces,
        activeWorkspaceId: planNav.activeWorkspaceId,
        canonicalProjectPath: resolved?.canonical_project_path,
        setActiveWorkspace,
        onMissing: undefined,
      });
    },
    [planIndex, planNav.workspaces, planNav.activeWorkspaceId, resolvedPlanRefs.refs, setActiveWorkspace],
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

  useEffect(() => {
    if (backendState?.state === "working" || pendingPrompt) {
      setLongIdle(false);
      return;
    }
    const id = window.setTimeout(() => setLongIdle(true), 90_000);
    return () => window.clearTimeout(id);
  }, [backendState?.state, pendingPrompt]);

  useEffect(() => {
    const completeQuestion = pendingPrompt?.question.trim() || backendState?.question?.trim() || "";
    const isActionableInputRequest = backendState?.state === "question"
      && backendState.subtype !== "rate_limit"
      && backendState.subtype !== "error";
    const generation = waitingQuestionTracker.observeAndNotify(
      tabId,
      agentSessionId,
      backendState?.state ?? null,
      completeQuestion || null,
      isActionableInputRequest && completeQuestion.length > 0,
      () => {
        void invoke("notify_agent_input_request", {
          tabId,
          sessionId: agentSessionId,
          promptIdentity: completeQuestion,
        }).catch((error) => {
          console.warn("Failed to create waiting-for-input notification:", error);
        });
      },
    );
    setWaitingGeneration(generation);
  }, [
    agentSessionId,
    backendState?.question,
    backendState?.state,
    backendState?.subtype,
    tabId,
    pendingPrompt?.question,
  ]);

  // subtype); it arrives via the mux snapshot and events, no local polling needed.
  const liveAgentState: AgentState = useMemo(() => {
    const kind = backendState?.state ?? "idle";
    return {
      state: kind,
      subtype: backendState?.subtype,
      status_line: backendState?.status_line,
      last_turn_summary: backendState?.last_turn_summary,
      phase: backendState?.phase,
      elapsed_ms: backendState?.elapsed_ms,
      longIdle: kind === "idle" && longIdle,
    };
  }, [
    backendState?.state,
    backendState?.status_line,
    backendState?.last_turn_summary,
    backendState?.phase,
    backendState?.elapsed_ms,
    backendState?.subtype,
    longIdle,
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

  if (loading) {
    return <div className="p-4 text-sm opacity-60">Loading chat…</div>;
  }

  return (
    <div
      ref={chatRootRef}
      className="w-full h-full flex flex-col relative"
      data-testid="tab-session-chat"
      data-tab-id={tabId}
      role="region"
      aria-label={`Tab chat: ${tab?.title ?? tabId}`}
      aria-busy={loading || recovering}
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
      <div className="relative flex-1 min-h-0">
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="absolute inset-0 overflow-y-auto overscroll-contain p-3 select-text"
        data-testid="tab-session-chat-messages"
        role="log"
        aria-label="Conversation"
        aria-live="polite"
      >
        <div ref={scrollContentRef}>
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
              onClick={() => void loadOlder()}
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
        {error && (
          <div className="py-8 text-center text-sm" data-testid="chat-recovery-error">
            <div className="mb-2">Chat history could not be loaded.</div>
            <div className="text-xs opacity-60">{error}</div>
            <button
              type="button"
              data-testid="chat-retry"
              onClick={() => void reattachTabChat(tabId).catch(() => retry())}
              className="mt-3 rounded px-3 py-1.5 text-xs hover:brightness-125"
              style={{ border: "1px solid var(--ct-border)", color: "var(--ct-fg)" }}
            >
              Reconnect
            </button>
          </div>
        )}
        {renderedEnrichedMessages.length === 0 && !inlinePrompt && liveAgentState.state !== "question" && (
          error ? null : recovering ? (
            <div className="opacity-50 text-center py-8 text-sm" data-testid="chat-recovering-state">
              Recovering chat history…
            </div>
          ) : bootSetupView.kind === "none" ? (
            <div className="opacity-50 text-center py-8 text-sm" data-testid="chat-empty-state">
              No messages yet.
            </div>
          ) : null
        )}
        {renderedEnrichedMessages.map(({ msg, item, origin }) => {
          const originProps = originPropsById.get(msg.id);
          return (
            <div
              key={msg.id}
              data-testid="chat-message"
              data-message-role={msg.type}
              data-message-id={msg.id}
              role="article"
              aria-label={chatMessageAccessibleName(msg.type, msg.content)}
            >
              <MessageBubble
                message={msg}
                item={item.kind === "tool" ? item : undefined}
                isLatestFailedToolCall={msg.id === latestFailedToolCallId}
                origin={origin}
                resolveTabName={resolveTabName}
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
          );
        })}
        <ChatSetupState
          tab={tab}
          view={bootSetupView}
          providerName={provider?.displayName ?? "the agent"}
          providerIconSvg={provider?.iconSvg}
          providerIconColor={provider?.iconColor}
          providerIconHint={provider?.iconHint}
          teamName={teamName}
          projectName={projectName}
          onRetry={handleLaunchRetry}
          onOpenTerminal={onOpenTerminalFallback}
          onEscape={focusComposer}
        />
        {!inlinePrompt && (renderedEnrichedMessages.length > 0 || liveAgentState.state === "question") && (
          // with the message text instead of the far-left container edge.
          <div className="w-full max-w-4xl min-w-0 mx-auto">
            <AgentStateIndicator
              state={liveAgentState}
              questionGeneration={waitingGeneration}
              providerName={provider?.displayName ?? null}
              model={workingModelBadge}
              onStop={effectiveChatBackend || !currentPtyId ? undefined : handleStopAgent}
              onCancel={effectiveChatBackend || !currentPtyId ? undefined : handleStopAgent}
              onOpenTerminal={effectiveChatBackend ? undefined : onOpenTerminalFallback}
            />
          </div>
        )}
        <PendingPromptCard pendingPrompt={inlinePrompt} onRespond={handleRespondPrompt} />
        </div>
      </div>
      {(showScrollDown || hiddenNewerCount > 0) && (
        <button
          type="button"
          data-testid="chat-scroll-bottom"
          onClick={() => {
            showLatest();
            scrollToBottom();
          }}
          title="Scroll to latest"
          aria-label={hiddenNewerCount > 0 ? `Show latest (${hiddenNewerCount} newer)` : "Scroll to latest"}
          className="absolute right-4 bottom-4 z-10 flex h-8 items-center justify-center gap-1 rounded-full px-2 text-[var(--ct-muted-fg)] transition-colors hover:text-[var(--ct-fg)]"
          style={{ background: "var(--ct-surface)", border: "1px solid var(--ct-border)" }}
        >
          {hiddenNewerCount > 0 && (
            <span className="text-[10px] font-mono" data-testid="chat-hidden-newer-count">
              {hiddenNewerCount}
            </span>
          )}
          <ChevronDown size={16} strokeWidth={2.2} />
        </button>
      )}
      </div>
      {isWatcherTab && watcherSpec ? (
        <WatcherReadOnlyBar
          watcher={watcherSpec}
          modelBubble={watcherModelBubble}
        />
      ) : (
      <>
      {renderedEnrichedMessages.length > 0 && (
        <ChatSetupStatusLine
          tab={tab}
          providerName={provider?.displayName ?? "the agent"}
        />
      )}
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
        showComposerError={showComposerError}
        onOpenTerminalFallback={effectiveChatBackend ? undefined : onOpenTerminalFallback}
        modelBubble={composerModelBubble}
      />
      </>
      )}
    </div>
  );
}
