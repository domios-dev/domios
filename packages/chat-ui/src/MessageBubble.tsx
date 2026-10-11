import React, { useState, useMemo, useCallback, useEffect, useDeferredValue, useLayoutEffect } from "react";
import type {
  ChatOrigin,
  NormalizedChatMessage as ChatMessage,
  TimelineItem,
} from "@codeterm/chat-core";
import { isTabMarkerPlaceholderLabel, shortTabRef, systemOriginLabel, toolCommand, userMessageAttachments } from "@codeterm/chat-core";
import {
  Send,            // generic chat (telegram / service)
  Network,         // mesh
  ChevronsRight,   // slash-command chip
  Settings,        // plugin_config (Configure card)
  Puzzle,          // plugin webhook fallback
  Info,
} from "lucide-react";
import { ToolCallBubble } from "./ToolCallBubble";
import { toolCommandShape } from "./codeterm/commandShape";
import { AttachmentStrip } from "./AttachmentStrip";
import { InlineAttachmentText } from "./AttachmentCapsule";
import { ReplyQuote } from "./ReplyQuote";
import { ThoughtLine } from "./toolcard/ThoughtLine";
import { ShellCommandItem } from "./ShellCommandItem";
import { ReminderCard } from "./codeterm/ReminderCard";
import { AutomationCard } from "./codeterm/AutomationCard";
import { InboundReportCard, parseInboundReport } from "./codeterm/InboundReportCard";
import { InboundMessageCard } from "./codeterm/InboundMessageCard";
import { CompactMessageContent } from "./codeterm/CompactMessageContent";
import { ChatNoticeCard } from "./ChatNoticeCard";
import { MessageTimeFrame } from "./MessageTimeFrame";
import { messageRowTime } from "./messageRowTime";
import type { PeerNameResolver, TabNameResolver } from "./codeterm/templates";
import { ChevronIcon, PRE_WRAP_CLASS } from "./CodeBlock";
import {
  CommandBody,
  ImageBadge,
  LongArtifact,
  RichMarkdown,
  TEXT_WRAP_CLASS,
  isLargeArtifact,
  renderUserContent,
} from "./MarkdownBody";
import { InteractiveElementRenderer } from "./interactive/InteractiveElementRenderer";
import type { InteractiveAnswerHandler } from "./interactive/InteractiveQuestion";
import { pickUiFontScaleValue } from "./uiFontScale";
import { ToolOutputContent } from "./ToolOutputContent";
import { isImageOnlyToolOutput } from "./toolImageOutput";
import { ContextRequestCard } from "./ContextRequestCard";
import { WatcherVerdictCard } from "./WatcherVerdictCard";
import { LinkedText, PlanRefCards, type ResolvedChatPlanRef } from "./codeterm/LinkedText";
import { hasPlanRefs } from "./codeterm/planRefs";
import { recognize } from "./codeterm/recognize";
import { isSend } from "./codeterm/templates";
import { parseCoordinationReport } from "./codeterm/coordinationReport";
import { parseDsTasksMessage } from "./codeterm/dsTasksBlock";
import { ChatBlockMessageProvider } from "./blocks/ChatBlockHost";
import { TasksBlock } from "./blocks/TasksBlock";
import { tasksBlockSource, tasksSpecFromBlock } from "./blocks/tasksBlockModel";
import { normalizedTimelineMarker, TimelineMarkerRow } from "./timelineMarkers";
import {
  hasMeaningfulToolResult,
  isRenderableMessage,
  structuredInboundFromMessage,
} from "./MessageBubble.helpers";

interface MessageBubbleProps {
  message: ChatMessage;
  streaming?: boolean;
  onContentLayout?: () => void;
  showTimestamp?: boolean;
  userFooter?: React.ReactNode;
  item?: TimelineItem;
  isLatestFailedToolCall?: boolean;
  /** Route an interactive widget's answer back to the agent (existing send path). */
  onAnswerInteractive?: InteractiveAnswerHandler;
  /** Widget ids already answered — kept in the parent so they survive remounts. */
  answeredInteractiveIds?: Set<string>;
  /** When set, this user message arrived via a codeterm channel (cross-tab,
   *  Telegram, plugin webhook, Pebble, mesh, slash-command) and should render as a typed header
   *  instead of the default right-aligned "user" styling. */
  origin?: ChatOrigin;
  originAvatar?: React.ReactNode;
  /** When set, clicking the origin header focuses the source tab. */
  onOriginClick?: () => void;
  /** Resolve a tab id/short-id to a tab name for CodeTerm cards (e.g. "Sent to <name>"). */
  resolveTabName?: TabNameResolver;
  /** Resolve a mesh UUID/name to the peer's human instance name. */
  resolvePeerName?: PeerNameResolver;
  /** Focus a referenced Tab from a CodeTerm action card. */
  onFocusTab?: (tabRef: string) => void;
  /** Open the plan view for a plan, optionally focused on a task/section node. */
  onOpenPlan?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => void;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  fontScale?: number;
  /** `shell_command` decoration — re-run the command in the tab's PTY. */
  onRerunShellCommand?: (command: string) => void;
  /** `shell_command` decoration — surface the command's terminal output. */
  onJumpToShellCommand?: (message: ChatMessage) => void;
  onStopReminder?: (id: string) => void;
  /** Opens the Jobs view for a cron or blueprint delivery card. */
  onOpenJobs?: () => void;
  /** Scroll to a quoted message; a user bubble's reply block calls it with the quoted id. */
  onJumpToMessage?: (messageId: string) => void;
}

const INTERACTIVE_QUESTIONS_ENABLED = false;
const BLOCK_GAP = "\n\n";
const TOOL_EXPANDED_BY_ID = new Map<string, boolean>();

function hasVisibleEdit(item: TimelineItem | undefined): boolean {
  return item?.kind === "tool"
    && item.event.kind === "edit"
    && Boolean(item.event.edits?.length);
}

// Transport ids are re-keyed when a snapshot replaces live rows, so tool expansion keys on source identity.
function expansionKey(message: ChatMessage, item: TimelineItem | undefined): string {
  if (item?.kind !== "tool") return message.id;
  const session = message.sourceSessionId ?? "";
  if (item.event.stream) return `stream:${session}:${item.event.stream.flowId}`;
  const toolId = item.event.toolId?.trim();
  return toolId ? `tool:${session}:${toolId}` : message.id;
}

function isOutboundMessage(item: TimelineItem | undefined): boolean {
  if (item?.kind !== "tool") return false;
  const { command, argv } = toolCommand(item.event);
  const invocation = recognize(command, argv);
  return invocation != null && isSend(invocation);
}

function tabOriginTitle(origin: Extract<ChatOrigin, { kind: "tab" }>, resolveTabName?: TabNameResolver): string {
  const tabRef = shortTabRef(origin.tabIdShort);
  const resolved = tabRef ? resolveTabName?.(tabRef) : undefined;
  if (isTabMarkerPlaceholderLabel(origin.title)) return resolved ?? (tabRef || "tab");
  return origin.title.trim() || resolved || tabRef || "tab";
}

// Test-only render counter (opt-in: a harness sets window.__bubbleRenderCounts
// to {}). No-op in production — a single property read on window.
function recordBubbleRender(id: string): void {
  if (typeof window === "undefined") return;
  const counts = (window as { __bubbleRenderCounts?: Record<string, number> }).__bubbleRenderCounts;
  if (!counts) return;
  counts[id] = (counts[id] ?? 0) + 1;
}

const MessageBubbleContent = React.memo(function MessageBubbleContent({ message, streaming = false, onContentLayout, userFooter, item, origin, originAvatar, onOriginClick, resolveTabName, resolvePeerName, onFocusTab, onOpenPlan, resolvePlanRef, onAnswerInteractive, answeredInteractiveIds, fontScale, onRerunShellCommand, onJumpToShellCommand, onStopReminder, onOpenJobs, onJumpToMessage }: MessageBubbleProps) {
  const renderingStream = useDeferredValue(streaming);
  useLayoutEffect(() => { onContentLayout?.(); });
  recordBubbleRender(message.id);
  const isInboundCommunication = origin?.kind === "tab" || origin?.kind === "pebble" || origin?.kind === "plugin" || message.inboundKind === "tab" || message.inboundKind === "report" || message.inboundKind === "mesh" || message.inboundKind === "pebble" || message.inboundKind === "plugin";
  const isCoordinationReport = message.inboundKind === "report" && parseCoordinationReport(message.content) != null;
  const isCompactStatusReport = message.inboundKind === "report" && /^(?:partial|in[ _-]?review)$/i.test(parseInboundReport(message.content).status ?? "");
  const isCommandGroup = item?.kind === "tool" && !item.event.stream
    && (item.event.kind === "command" || item.event.kind === "codeterm")
    && toolCommandShape(item.event)?.kind === "group";
  const shouldAutoExpand = isCommandGroup || hasVisibleEdit(item) || (isInboundCommunication && !isCoordinationReport && !isCompactStatusReport) || isOutboundMessage(item);
  const persistKey = expansionKey(message, item);
  const [expanded, setExpanded] = useState(() => {
    const persisted = TOOL_EXPANDED_BY_ID.get(persistKey);
    return persisted ?? shouldAutoExpand;
  });

  const renderedContent = useMemo(() => {
    if (message.type === "assistant") {
      if (renderingStream) return <div className="whitespace-pre-wrap">{message.content}</div>;
      return (
        <>
          <ChatBlockMessageProvider messageId={message.id}>
            <RichMarkdown text={message.content} fontScale={fontScale} />
          </ChatBlockMessageProvider>
          <PlanRefCards text={message.content} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />
        </>
      );
    }
    return null;
  }, [message.id, message.type, message.content, renderingStream, fontScale, onOpenPlan, resolvePlanRef]);

  useEffect(() => {
    const persisted = TOOL_EXPANDED_BY_ID.get(persistKey);
    if (persisted != null) {
      setExpanded(persisted);
      return;
    }
    if (shouldAutoExpand) {
      setExpanded(true);
      TOOL_EXPANDED_BY_ID.set(persistKey, true);
    } else {
      setExpanded(false);
    }
  }, [persistKey, shouldAutoExpand]);

  const setExpandedPersistent = useCallback((next: boolean) => {
    TOOL_EXPANDED_BY_ID.set(persistKey, next);
    setExpanded(next);
  }, [persistKey]);

  const timelineMarker = normalizedTimelineMarker(message);
  if (timelineMarker?.kind === "context_compaction" || timelineMarker?.kind === "external_edit") {
    return <TimelineMarkerRow message={message} />;
  }

  if (!isRenderableMessage(message)) {
    return null;
  }

  if (message.type === "image") {
    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
        <ImageBadge />
      </div>
    );
  }

  if (message.type === "context_request") {
    return <ContextRequestCard content={message.content} fontScale={fontScale} />;
  }

  if (message.type === "watcher_verdict") {
    return <WatcherVerdictCard content={message.content} />;
  }

  if (message.type === "user") {
    const attachments = message.attachments;
    const userText = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
    const structuredInbound = structuredInboundFromMessage(message);
    const effectiveOrigin = origin ?? structuredInbound?.origin;
    const inboundKind = structuredInbound?.kind;

    if (effectiveOrigin?.kind === "system_prompt" || effectiveOrigin?.kind === "system") {
      const prompt = effectiveOrigin.kind === "system_prompt";
      return (
        <ChatNoticeCard
          testId={prompt ? "chat-system-prompt-card" : "chat-system-event"}
          role={prompt ? undefined : "status"}
          tone={prompt ? "var(--ct-green)" : undefined}
          icon={<Info size={14} strokeWidth={2} aria-hidden="true" />}
          title={prompt ? "System prompt" : systemOriginLabel(effectiveOrigin.context)}
          subtitle={prompt ? "This is the shell system prompt." : undefined}
          expanded={expanded}
          onToggle={() => setExpandedPersistent(!expanded)}
          bodyClassName={`${userText} ${TEXT_WRAP_CLASS}`}
        >
          <CompactMessageContent text={message.content} fontScale={fontScale} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} collapseAfter={Number.POSITIVE_INFINITY} />
        </ChatNoticeCard>
      );
    }

    if (effectiveOrigin?.kind === "automation") {
      const tabSender = effectiveOrigin.source === "utility" || effectiveOrigin.report;
      return (
        <AutomationCard
          origin={effectiveOrigin}
          onOpen={tabSender ? onOriginClick : onOpenJobs}
          openLabel={tabSender ? "Open tab" : "Open job"}
        >
          <CompactMessageContent text={message.content} fontScale={fontScale} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} collapseAfter={Number.POSITIVE_INFINITY} />
        </AutomationCard>
      );
    }

    if (effectiveOrigin?.kind === "reminder") {
      return (
        <ReminderCard
          whoSet={effectiveOrigin.whoSet}
          model={effectiveOrigin.model}
          setAt={effectiveOrigin.setAt}
          fireAt={effectiveOrigin.fireAt}
          id={effectiveOrigin.id}
          everyMs={effectiveOrigin.everyMs}
          nextAt={effectiveOrigin.nextAt}
          skippedTicks={effectiveOrigin.skippedTicks}
          onStop={onStopReminder}
          body={message.content}
        />
      );
    }

    if (effectiveOrigin?.kind === "tab" && effectiveOrigin.report) {
      return (
        <InboundReportCard
          message={message}
          sender={tabOriginTitle(effectiveOrigin, resolveTabName)}
          expanded={expanded}
          onToggle={() => setExpandedPersistent(!expanded)}
          onFocusTab={onOriginClick}
          onOpenPlan={onOpenPlan}
          resolvePlanRef={resolvePlanRef}
          fontScale={fontScale}
        />
      );
    }

    if (effectiveOrigin?.kind === "tab") {
      return (
        <InboundMessageCard
          message={message}
          sender={tabOriginTitle(effectiveOrigin, resolveTabName)}
          model={effectiveOrigin.model}
          channel={inboundKind === "mesh" || effectiveOrigin.mesh ? "mesh" : "tab"}
          expanded={expanded}
          onToggle={() => setExpandedPersistent(!expanded)}
          onFocusTab={onOriginClick}
          onOpenPlan={onOpenPlan}
          resolvePlanRef={resolvePlanRef}
          fontScale={fontScale}
        />
      );
    }

    // codeterm channel (cross-tab, Telegram, plugin, Pebble, mesh) or wraps slash-commands.
    const originHeader = effectiveOrigin ? (() => {
      const OriginIcon =
        effectiveOrigin.kind === "mesh"          ? Network :
        effectiveOrigin.kind === "command"       ? ChevronsRight :
        effectiveOrigin.kind === "plugin_config" ? Settings :
        effectiveOrigin.kind === "plugin"       ? Puzzle :
        Send;
      const label =
        effectiveOrigin.kind === "telegram"     ? `Telegram · ${effectiveOrigin.userId.slice(0, 12)}` :
        effectiveOrigin.kind === "pebble"       ? "Pebble" :
        effectiveOrigin.kind === "plugin"       ? effectiveOrigin.name || effectiveOrigin.pluginId :
        effectiveOrigin.kind === "mesh"         ? `Mesh · ${effectiveOrigin.peerId.slice(0, 10)}` :
        effectiveOrigin.kind === "plugin_config"? `Configure · ${effectiveOrigin.name}` :
        effectiveOrigin.kind === "command" ? (
          effectiveOrigin.commands.length === 1
            ? effectiveOrigin.commands[0].name
            : `${effectiveOrigin.commands.length} commands`
        ) :
        `${effectiveOrigin.service} · ${effectiveOrigin.userId.slice(0, 12)}`;
      return (
        <button
          type="button"
          onClick={onOriginClick}
          disabled={!onOriginClick}
          className="mb-1 flex items-center gap-1.5 text-[11.5px] font-medium"
          style={{
            color: "color-mix(in srgb, var(--ct-accent) 72%, var(--ct-muted-fg))",
            cursor: onOriginClick ? "pointer" : "default",
            background: "transparent",
            border: "none",
            padding: 0,
          }}
          title={onOriginClick ? "Click to focus source Tab" : undefined}
        >
          {effectiveOrigin.kind === "plugin" ? (originAvatar ?? <Puzzle size={12} aria-hidden="true" />) : <OriginIcon size={12} aria-hidden="true" />}
          <span>{label}</span>
        </button>
      );
    })() : null;

    const dsTasks = effectiveOrigin ? null : parseDsTasksMessage(message.content);
    const tasksSpec = dsTasks ? tasksSpecFromBlock(dsTasks.block) : null;
    const userContent = dsTasks ? [dsTasks.before, dsTasks.after].filter(Boolean).join(BLOCK_GAP) : message.content;
    const userAttachments = userMessageAttachments(userContent, attachments);
    const visibleUserContent = userAttachments.text;
    const formattedUserContent = effectiveOrigin?.kind === "command" ? (
          <CommandBody commands={effectiveOrigin.commands} body={message.content} />
        ) : userAttachments.inline ? (
          <InlineAttachmentText segments={userAttachments.inline} />
        ) : hasPlanRefs(visibleUserContent) ? (
          <LinkedText text={visibleUserContent} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />
        ) : (
          visibleUserContent ? renderUserContent(visibleUserContent) : null
        );

    const reply = userAttachments.reply;
    const quotedId = reply?.reply;
    const userBubbleInner = (
      <>
        {reply && (
          <ReplyQuote
            label={reply.label ?? "Quote"}
            excerpt={reply.excerpt ?? ""}
            onOpen={onJumpToMessage && quotedId ? () => onJumpToMessage(quotedId) : undefined}
          />
        )}
        {(userAttachments.images.length > 0 || userAttachments.files.length > 0) && (
          <AttachmentStrip images={userAttachments.images} files={userAttachments.files} captioned={visibleUserContent.trim().length > 0 || userAttachments.inline !== null} />
        )}
        {formattedUserContent}
        {userFooter}
      </>
    );

    const showUserBubble = Boolean(userFooter) || !dsTasks || visibleUserContent.trim().length > 0 || Boolean(attachments?.length) || reply !== null;

    return (
      <div className="ct-chat-message-row ct-chat-message-row--user mb-3 w-full min-w-0">
        <div className="ct-chat-message-column w-full max-w-4xl min-w-0 mx-auto">
          <div className={`group flex w-full min-w-0 ${effectiveOrigin ? "justify-start" : "justify-end"}`}>
            <div className="relative min-w-0 w-full max-w-full sm:w-auto sm:max-w-[75%]">
              {originHeader}
              {showUserBubble && <div
                className={`ct-user-bubble${effectiveOrigin ? " ct-user-bubble--relayed" : ""} min-w-0 px-4 py-2 ${userText} select-text cursor-text ${TEXT_WRAP_CLASS}`}
              >
                {userBubbleInner}
              </div>}
            </div>
          </div>
          {tasksSpec && (
            <div className="flex w-full min-w-0 justify-end">
              <TasksBlock spec={tasksSpec} source={tasksBlockSource(tasksSpec)} />
            </div>
          )}
        </div>
      </div>
    );
  }

  if (message.type === "assistant") {
    const assistantText = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
    return (
      <div className="ct-chat-message-row ct-chat-message-row--agent mb-3 w-full min-w-0">
        <div className="ct-chat-message-column relative w-full max-w-4xl min-w-0 mx-auto">
          <div
            className={`min-w-0 max-w-full ${assistantText} leading-relaxed select-text cursor-text ${TEXT_WRAP_CLASS}`}
            style={{ color: "var(--ct-fg)" }}
          >
            {renderedContent}
          </div>
        </div>
      </div>
    );
  }

  if (message.type === "system" || message.type === "local_command") {
    if (message.systemSubtype === "api_error") {
      return <div role="alert" aria-label="Provider API error" className="mb-2 w-full max-w-4xl mx-auto rounded-md border p-3 text-sm" style={{ color: "var(--ct-red)", borderColor: "var(--ct-red)" }}>{message.content}</div>;
    }
    if (message.systemSubtype === "context_epoch_boundary") {
      return (
        <div
          role="separator"
          aria-label={message.content}
          data-testid="chat-context-epoch-divider"
          data-context-epoch={message.contextEpoch}
          className="mb-3 flex w-full max-w-4xl min-w-0 items-center gap-3 mx-auto"
          style={{ color: "var(--ct-muted-fg)" }}
        >
          <span className="h-px flex-1" style={{ background: "var(--ct-border)" }} />
          <span className="max-w-[70%] text-center text-[11px]">{message.content}</span>
          <span className="h-px flex-1" style={{ background: "var(--ct-border)" }} />
        </div>
      );
    }
    const label = message.tagName || message.systemSubtype || "system";
    const isCaveat = message.tagName === "local-command-caveat";
    const isSkillContext = message.systemSubtype === "skill_context";
    const isCompactSummary = message.systemSubtype === "compact_summary" || message.isCompactSummary === true;
    const isAgentInstructions = message.systemSubtype === "agent_instructions";
    const isEnvironmentContext = message.systemSubtype === "environment_context";
    const isTurnAborted = message.systemSubtype === "turn_aborted";
    if (isCompactSummary) {
      return (
        <LongArtifact
          label="Session continued from summary"
          text={message.content}
          expanded={expanded}
          onToggle={() => setExpandedPersistent(!expanded)}
          markdown
        />
      );
    }
    if (isTurnAborted) {
      // chat turn. Render as the centered status chip, never the raw XML text.
      return (
        <div className="flex w-full max-w-4xl min-w-0 justify-center mb-2 mx-auto">
          <span
            className="inline-flex items-center gap-1 text-[10px] px-2.5 py-0.5 rounded-full"
            style={{ color: "var(--ct-muted-fg)", background: "var(--ct-surface)" }}
          >
            ⊘ Turn interrupted
          </span>
        </div>
      );
    }
    if (isAgentInstructions || isEnvironmentContext) {
      return (
        <LongArtifact
          label={isEnvironmentContext ? "Environment context" : "System context"}
          text={message.content}
          expanded={expanded}
          onToggle={() => setExpandedPersistent(!expanded)}
          markdown
        />
      );
    }
    if (isSkillContext || isLargeArtifact(message.content)) {
      return (
        <LongArtifact
          label={isSkillContext ? "skill context" : label}
          text={message.content}
          expanded={expanded}
          onToggle={() => setExpandedPersistent(!expanded)}
          markdown={isSkillContext || /\.md\b/i.test(label)}
        />
      );
    }
    if (!isCaveat && message.content) {
      return (
        <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
          <ThoughtLine
            text={message.content}
            label={label}
            testId="chat-system-meta"
            expanded={expanded}
            onToggle={setExpandedPersistent}
          />
        </div>
      );
    }
    return (
      <div className="mb-2 w-full max-w-4xl min-w-0 mx-auto">
        <div
          className="inline-flex max-w-full min-w-0 flex-col gap-1 rounded-md px-2.5 py-1.5 text-xs border"
          style={{
            background: isCaveat
              ? "color-mix(in srgb, var(--ct-yellow, #d29922) 7%, var(--ct-surface, #161b22) 93%)"
              : "var(--ct-surface)",
            borderColor: isCaveat
              ? "color-mix(in srgb, var(--ct-yellow, #d29922) 25%, transparent)"
              : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 20%, transparent)",
            color: "var(--ct-muted-fg)",
          }}
        >
          <span
            className="inline-flex items-center gap-1.5 font-mono text-[11px]"
            style={{ color: isCaveat ? "var(--ct-yellow, #d29922)" : "var(--ct-accent, #58a6ff)" }}
          >
            <Info size={12} strokeWidth={2} />
            {label}
          </span>
          {message.content && (
            <pre className={`${PRE_WRAP_CLASS} font-mono m-0 select-text`} style={{ color: "var(--ct-fg)" }}>
              {message.content}
            </pre>
          )}
        </div>
      </div>
    );
  }

  if (INTERACTIVE_QUESTIONS_ENABLED && message.interactive && message.interactive.length > 0) {
    return (
      <div className="w-full">
        {message.interactive.map((el) => (
          <InteractiveElementRenderer
            key={el.id}
            element={el}
            onAnswer={onAnswerInteractive}
            answered={answeredInteractiveIds?.has(el.id)}
          />
        ))}
      </div>
    );
  }

  if (item?.kind === "tool" && item.event.agents) {
    return <ToolCallBubble event={item.event} expanded={expanded} onToggle={setExpandedPersistent} />;
  }

  if (message.type === "tool_call") {
    return (
        <ToolCallBubble
          event={item?.kind === "tool" ? item.event : undefined}
          message={item?.kind === "tool" ? undefined : message}
          expanded={expanded}
          onToggle={setExpandedPersistent}
          resolveTabName={resolveTabName}
          resolvePeerName={resolvePeerName}
          onFocusTab={onFocusTab}
          onOpenPlan={onOpenPlan}
          resolvePlanRef={resolvePlanRef}
          fontScale={fontScale}
        />
    );
  }

  if (message.type === "tool_result") {
    if (!hasMeaningfulToolResult(message.content)) return null;
    const standaloneResult = message.toolResult || message.content;
    if (isImageOnlyToolOutput(standaloneResult)) {
      return (
        <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto pl-4">
          <ToolOutputContent text={standaloneResult} />
        </div>
      );
    }
    if (isLargeArtifact(message.content)) {
      return (
        <LongArtifact
          label="tool result"
          text={message.content}
          expanded={expanded}
          onToggle={() => setExpandedPersistent(!expanded)}
        />
      );
    }
    const contentLines = message.content.split("\n");
    const preview = contentLines.slice(0, 2).join("\n");
    const hasMore = contentLines.length > 2;

    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto pl-4">
        <button
          onClick={() => setExpandedPersistent(!expanded)}
          className="inline-flex max-w-full min-w-0 items-start gap-1.5 text-left rounded-md px-2.5 py-1 text-xs transition-colors duration-150 hover:brightness-110 sm:max-w-md"
          style={{
            color: "var(--ct-muted-fg)",
          }}
        >
          <span className="opacity-40 flex-shrink-0 mt-0.5 text-[10px]">{"\u2192"}</span>
          <span
            className="opacity-60 font-mono text-[11px] overflow-hidden"
            style={{ display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical" as const }}
          >
            {preview}
            {hasMore && !expanded ? "..." : ""}
          </span>
          {hasMore && <ChevronIcon expanded={expanded} className="ml-1 mt-0.5 opacity-40" />}
        </button>
        {expanded && (
          <div className="mt-1 ml-4 max-w-full min-w-0">
            <ToolOutputContent text={standaloneResult} maxHeight="12rem" />
          </div>
        )}
      </div>
    );
  }

  if (message.type === "thinking") {
    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
        <ThoughtLine text={message.content} expanded={expanded} onToggle={setExpandedPersistent} />
      </div>
    );
  }

  // Shell command — first-class command item with VS Code-style decorations.
  if (message.type === "shell_command") {
    return (
      <ShellCommandItem
        message={message}
        onRerun={onRerunShellCommand}
        onJump={onJumpToShellCommand}
      />
    );
  }

  if (message.type === "status") {
    return (
      <div className="flex w-full max-w-4xl min-w-0 justify-center mb-2 mx-auto">
        <span
          className="text-[10px] px-2.5 py-0.5 rounded-full"
          style={{
            color: "var(--ct-muted-fg)",
            background: "var(--ct-surface)",
          }}
        >
          {message.content}
        </span>
      </div>
    );
  }

  return null;
});

export const MessageBubble = React.memo(function MessageBubble(props: MessageBubbleProps) {
  const time = messageRowTime(props.message, props.item, props.origin);
  const content = <MessageBubbleContent {...props} />;
  if (props.showTimestamp === false || !time || !isRenderableMessage(props.message)) return content;
  return <MessageTimeFrame timestamp={String(time.timestamp)} side={time.side}>{content}</MessageTimeFrame>;
});
