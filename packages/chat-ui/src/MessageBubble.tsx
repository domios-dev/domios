import React, { useState, useMemo, useCallback, useEffect } from "react";
import type {
  ChatOrigin,
  NormalizedChatMessage as ChatMessage,
  ChatAttachment as ImageAttachment,
  TimelineItem,
} from "@codeterm/chat-core";
import { isTabMarkerPlaceholderLabel, shortTabRef } from "@codeterm/chat-core";
import {
  Download, Image,
  Send,            // generic chat (telegram / service)
  Network,         // mesh
  ChevronsRight,   // slash-command chip
  Settings,        // plugin_config (Configure card)
  Info,
} from "lucide-react";
import { ToolCallBubble } from "./ToolCallBubble";
import { ThoughtLine } from "./toolcard/ThoughtLine";
import { ShellCommandItem } from "./ShellCommandItem";
import { ReminderCard } from "./codeterm/ReminderCard";
import { InboundReportCard, parseInboundReport } from "./codeterm/InboundReportCard";
import { InboundMessageCard } from "./codeterm/InboundMessageCard";
import { messageTimestamp } from "./codeterm/relativeTime";
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
import {
  hasMeaningfulToolResult,
  isRenderableMessage,
  structuredInboundFromMessage,
} from "./MessageBubble.helpers";

interface MessageBubbleProps {
  message: ChatMessage;
  item?: TimelineItem;
  isLatestFailedToolCall?: boolean;
  /** Route an interactive widget's answer back to the agent (existing send path). */
  onAnswerInteractive?: InteractiveAnswerHandler;
  /** Widget ids already answered — kept in the parent so they survive remounts. */
  answeredInteractiveIds?: Set<string>;
  /** When set, this user message arrived via a codeterm channel (cross-tab,
   *  telegram, mesh, slash-command) and should render as a typed header
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
}

const INTERACTIVE_QUESTIONS_ENABLED = false;
const TOOL_EXPANDED_BY_ID = new Map<string, boolean>();

function hasVisibleEdit(item: TimelineItem | undefined): boolean {
  return item?.kind === "tool"
    && item.event.kind === "edit"
    && Boolean(item.event.edits?.length);
}

function isOutboundMessage(item: TimelineItem | undefined): boolean {
  if (item?.kind !== "tool") return false;
  const command = item.event.detail?.trim() || item.event.summary || item.event.result?.trim() || "";
  const invocation = recognize(command);
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

export const MessageBubble = React.memo(function MessageBubble({ message, item, origin, onOriginClick, resolveTabName, resolvePeerName, onFocusTab, onOpenPlan, resolvePlanRef, onAnswerInteractive, answeredInteractiveIds, fontScale, onRerunShellCommand, onJumpToShellCommand }: MessageBubbleProps) {
  recordBubbleRender(message.id);
  const isInboundCommunication = origin?.kind === "tab" || message.inboundKind === "tab" || message.inboundKind === "report" || message.inboundKind === "mesh";
  const isCoordinationReport = message.inboundKind === "report" && parseCoordinationReport(message.content) != null;
  const isCompactStatusReport = message.inboundKind === "report" && /^(?:partial|in[ _-]?review)$/i.test(parseInboundReport(message.content).status ?? "");
  const shouldAutoExpand = hasVisibleEdit(item) || (isInboundCommunication && !isCoordinationReport && !isCompactStatusReport) || isOutboundMessage(item);
  const [expanded, setExpanded] = useState(() => {
    const persisted = TOOL_EXPANDED_BY_ID.get(message.id);
    return persisted ?? shouldAutoExpand;
  });
  const [viewingImage, setViewingImage] = useState<ImageAttachment | null>(null);

  const renderedContent = useMemo(() => {
    if (message.type === "assistant") {
      return (
        <>
          <RichMarkdown text={message.content} fontScale={fontScale} />
          <PlanRefCards text={message.content} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />
        </>
      );
    }
    return null;
  }, [message.type, message.content, fontScale, onOpenPlan, resolvePlanRef]);

  const handleImageClick = useCallback((att: ImageAttachment) => {
    setViewingImage(att);
  }, []);

  useEffect(() => {
    const persisted = TOOL_EXPANDED_BY_ID.get(message.id);
    if (persisted != null) {
      setExpanded(persisted);
      return;
    }
    if (shouldAutoExpand) {
      setExpanded(true);
      TOOL_EXPANDED_BY_ID.set(message.id, true);
    } else {
      setExpanded(false);
    }
  }, [message.id, shouldAutoExpand]);

  const setExpandedPersistent = useCallback((next: boolean) => {
    TOOL_EXPANDED_BY_ID.set(message.id, next);
    setExpanded(next);
  }, [message.id]);

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

    if (effectiveOrigin?.kind === "system_prompt") {
      return (
        <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto">
          <div
            data-testid="chat-system-prompt-card"
            className="max-w-full min-w-0 rounded-md border"
            style={{
              background: "color-mix(in srgb, var(--ct-surface, #161b22) 92%, var(--ct-green, #3fb950) 8%)",
              borderColor: "color-mix(in srgb, var(--ct-green, #3fb950) 32%, transparent)",
              color: "var(--ct-fg)",
            }}
          >
            <button
              type="button"
              onClick={() => setExpandedPersistent(!expanded)}
              className="flex w-full min-w-0 items-center gap-2 px-3 py-2 text-left transition-colors hover:brightness-110"
              style={{ color: "var(--ct-fg)" }}
            >
              <Info size={14} strokeWidth={2} style={{ color: "var(--ct-green, #3fb950)" }} />
              <span className="min-w-0 flex-1">
                <span className="block text-xs font-medium">System prompt</span>
                <span className="block truncate text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
                  This is the shell system prompt.
                </span>
              </span>
              <ChevronIcon expanded={expanded} className="opacity-60" />
            </button>
            {expanded && (
              <div
                className={`border-t px-3 py-2 ${userText} ${TEXT_WRAP_CLASS}`}
                style={{ borderColor: "color-mix(in srgb, var(--ct-green, #3fb950) 22%, transparent)" }}
              >
                <RichMarkdown text={message.content} fontScale={fontScale} />
              </div>
            )}
          </div>
        </div>
      );
    }

    if (effectiveOrigin?.kind === "system") {
      const title = effectiveOrigin.context ? `System · ${effectiveOrigin.context}` : "System";
      return (
        <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto" role="status">
          <div
            data-testid="chat-system-event"
            className="max-w-full min-w-0 rounded-md border"
            style={{
              background: "color-mix(in srgb, var(--ct-surface, #161b22) 94%, var(--ct-accent, #58a6ff) 6%)",
              borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 28%, transparent)",
              color: "var(--ct-fg)",
            }}
          >
            <button
              type="button"
              aria-expanded={expanded}
              onClick={() => setExpandedPersistent(!expanded)}
              className="flex w-full min-w-0 items-center gap-2 px-3 py-2 text-left transition-colors hover:brightness-110"
            >
              <Info size={14} aria-hidden="true" />
              <span className="min-w-0 flex-1 truncate text-xs font-medium">{title}</span>
              <ChevronIcon expanded={expanded} className="opacity-60" />
            </button>
            {expanded && (
              <div
                className={`border-t px-3 py-2 ${userText} ${TEXT_WRAP_CLASS}`}
                style={{ borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 20%, transparent)" }}
              >
                <RichMarkdown text={message.content} fontScale={fontScale} />
              </div>
            )}
          </div>
        </div>
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

    // codeterm channel (cross-tab, telegram, mesh) or wraps slash-commands.
    const originHeader = effectiveOrigin ? (() => {
      const OriginIcon =
        effectiveOrigin.kind === "mesh"          ? Network :
        effectiveOrigin.kind === "command"       ? ChevronsRight :
        effectiveOrigin.kind === "plugin_config" ? Settings :
        Send;
      const label =
        effectiveOrigin.kind === "telegram"     ? `Telegram · ${effectiveOrigin.userId.slice(0, 12)}` :
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
          className="flex items-center gap-1.5 text-xs mb-1 uppercase tracking-wider opacity-80"
          style={{
            color: "var(--ct-accent, #58a6ff)",
            cursor: onOriginClick ? "pointer" : "default",
            background: "transparent",
            border: "none",
            padding: 0,
            textTransform: "uppercase",
          }}
          title={onOriginClick ? "Click to focus source Tab" : undefined}
        >
          <OriginIcon size={12} />
          <span>{label}</span>
        </button>
      );
    })() : null;

    const formattedUserContent = effectiveOrigin?.kind === "command" ? (
          <CommandBody commands={effectiveOrigin.commands} body={message.content} />
        ) : hasPlanRefs(message.content) ? (
          <LinkedText text={message.content} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />
        ) : (
          renderUserContent(message.content)
        );

    const userBubbleInner = (
      <>
        {formattedUserContent}
        {attachments && attachments.length > 0 && (
          <div className="flex flex-wrap gap-1.5 mt-2 pt-2" style={{ borderTop: "1px solid color-mix(in srgb, var(--ct-accent) 15%, transparent)" }}>
            {attachments.map((att) => (
              <button
                key={att.id}
                data-testid="chat-image-attachment-button"
                onClick={() => handleImageClick(att)}
                className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] transition-colors hover:brightness-125"
                style={{
                  background: "color-mix(in srgb, var(--ct-accent) 15%, transparent)",
                  color: "var(--ct-muted-fg)",
                }}
                title={att.data ? "Click to preview" : "Image (not loaded)"}
              >
                <Image size={12} strokeWidth={2} />
                {att.mimeType.replace("image/", "").toUpperCase()}
              </button>
            ))}
          </div>
        )}
        {viewingImage && viewingImage.data && (
          <div
            data-testid="chat-image-preview"
            className="mt-2 pt-2"
            style={{ borderTop: "1px solid color-mix(in srgb, var(--ct-accent) 15%, transparent)" }}
          >
            <img
              src={`data:${viewingImage.mimeType};base64,${viewingImage.data}`}
              alt="Attached image"
              className="max-w-full max-h-[300px] object-contain rounded-lg"
            />
            <div className="flex items-center gap-2 mt-1.5">
              <a
                href={`data:${viewingImage.mimeType};base64,${viewingImage.data}`}
                download={`image.${viewingImage.mimeType.replace("image/", "")}`}
                className="inline-flex items-center gap-1 text-[11px] px-1.5 py-0.5 rounded transition-colors hover:brightness-125"
                style={{ color: "var(--ct-muted-fg)", background: "color-mix(in srgb, var(--ct-accent) 15%, transparent)" }}
              >
                <Download size={11} strokeWidth={2} />
                Save
              </a>
              <button
                onClick={() => setViewingImage(null)}
                className="text-[11px] px-1.5 py-0.5 rounded transition-colors hover:brightness-125"
                style={{ color: "var(--ct-muted-fg)" }}
              >
                Hide
              </button>
            </div>
          </div>
        )}
      </>
    );

    const timestampFooter = (() => {
      const ts = messageTimestamp(message.timestamp);
      if (!ts) return null;
      return (
        <div
          className={`mt-0.5 text-[10px] opacity-0 transition-opacity duration-150 group-hover:opacity-60 ${effectiveOrigin ? "text-left" : "text-right"}`}
          style={{ color: "var(--ct-muted-fg)" }}
          title={ts.full}
        >
          {ts.short}
        </div>
      );
    })();

    return (
      <div className={`group flex mb-3 w-full max-w-4xl min-w-0 mx-auto ${effectiveOrigin ? "justify-start" : "justify-end"}`}>
        <div className="min-w-0 max-w-[75%] sm:max-w-[75%]">
          {originHeader}
          <div
            className={`min-w-0 rounded-2xl rounded-br-sm px-4 py-2 ${userText} select-text cursor-text border ${TEXT_WRAP_CLASS}`}
            style={{
              background: effectiveOrigin
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, var(--ct-surface, #161b22) 88%)"
                : "color-mix(in srgb, var(--ct-accent, #58a6ff) 22%, var(--ct-bg, #0d1117) 78%)",
              borderColor: effectiveOrigin
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)"
                : "color-mix(in srgb, var(--ct-accent, #58a6ff) 45%, transparent)",
              color: "var(--ct-fg)",
            }}
          >
            {userBubbleInner}
          </div>
          {timestampFooter}
        </div>
      </div>
    );
  }

  if (message.type === "assistant") {
    const assistantText = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
    return (
      <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto">
        <div
          className={`min-w-0 max-w-full ${assistantText} leading-relaxed select-text cursor-text ${TEXT_WRAP_CLASS}`}
          style={{ color: "var(--ct-fg)" }}
        >
          {renderedContent}
        </div>
      </div>
    );
  }

  if (message.type === "system" || message.type === "local_command") {
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
