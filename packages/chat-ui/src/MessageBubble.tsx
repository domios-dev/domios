import React, { useState, useMemo, useCallback, useEffect } from "react";
import type {
  ChatOrigin,
  NormalizedChatMessage as ChatMessage,
  ChatAttachment as ImageAttachment,
} from "@codeterm/chat-core";
import {
  Bot,
  Download, FileText, Image,
  Pencil,
  Search,
  Terminal,
  ArrowRight,      // pane -> pane
  Send,            // generic chat (telegram / service)
  Network,         // mesh
  ChevronsRight,   // slash-command chip
  CornerUpLeft,    // report (bubbled up to parent)
  Settings,        // plugin_config (Configure card)
  Info,
} from "lucide-react";
import { ShellCommandItem } from "./ShellCommandItem";
import { recognize } from "./codeterm/recognize";
import { CodetermCard } from "./codeterm/CodetermCard";
import { ReminderCard } from "./codeterm/ReminderCard";
import { messageTimestamp } from "./codeterm/relativeTime";
import type { PaneNameResolver } from "./codeterm/templates";
import { CodeBlock, ChevronIcon, PRE_WRAP_CLASS, lineCount } from "./CodeBlock";
import {
  CommandBody,
  DiffView,
  ImageBadge,
  LongArtifact,
  ModelAvatar,
  RichMarkdown,
  TEXT_WRAP_CLASS,
  extractCommand,
  extractEdits,
  extractFilePath,
  inferCodeLanguage,
  isLargeArtifact,
  languageForEdits,
  languageFromPath,
  renderUserContent,
  shortPath,
} from "./MarkdownBody";
import { ToolIcon } from "./toolIcons";
import { InteractiveElementRenderer } from "./interactive/InteractiveElementRenderer";
import type { InteractiveAnswerHandler } from "./interactive/InteractiveQuestion";
import { pickUiFontScaleValue } from "./uiFontScale";
import { ToolOutputContent } from "./ToolOutputContent";
import { isImageOnlyToolOutput, toolResultPreviewLine } from "./toolImageOutput";
import { ContextRequestCard } from "./ContextRequestCard";
import { WatcherVerdictCard } from "./WatcherVerdictCard";
import {
  INBOUND_KIND_LABELS,
  contentSize,
  hasMeaningfulToolResult,
  isRenderableMessage,
  parsedToolInput,
  previewLine,
  resolveCuratedToolKind,
  structuredInboundFromMessage,
  textArg,
  toolDisplayLabel,
} from "./MessageBubble.helpers";
import { CrossPaneMessageShell } from "./crossPaneBubble";

interface MessageBubbleProps {
  message: ChatMessage;
  /** Route an interactive widget's answer back to the agent (existing send path). */
  onAnswerInteractive?: InteractiveAnswerHandler;
  /** Widget ids already answered — kept in the parent so they survive remounts. */
  answeredInteractiveIds?: Set<string>;
  /** When set, this user message arrived via a codeterm channel (cross-pane,
   *  telegram, mesh, slash-command) and should render as a typed header
   *  instead of the default right-aligned "user" styling. */
  origin?: ChatOrigin;
  originAvatar?: React.ReactNode;
  /** When set, clicking the origin header focuses the source pane. */
  onOriginClick?: () => void;
  /** Resolve a pane id/short-id to a tab name for CodeTerm cards (e.g. "Sent to <name>"). */
  resolvePaneName?: PaneNameResolver;
  /** Open the plan view for a plan (optionally focused on a task) from chat cards/links. */
  onOpenPlan?: (planId: string, taskId?: string) => void;
  fontScale?: number;
  /** `shell_command` decoration — re-run the command in the pane's PTY. */
  onRerunShellCommand?: (command: string) => void;
  /** `shell_command` decoration — surface the command's terminal output. */
  onJumpToShellCommand?: (message: ChatMessage) => void;
}

const INTERACTIVE_QUESTIONS_ENABLED = false;

export const MessageBubble = React.memo(function MessageBubble({ message, origin, originAvatar, onOriginClick, resolvePaneName, onOpenPlan, onAnswerInteractive, answeredInteractiveIds, fontScale, onRerunShellCommand, onJumpToShellCommand }: MessageBubbleProps) {
  const shouldAutoExpand = message.type === "tool_call"
    && (message.toolInput?.content != null || extractEdits(message).length > 0);
  const [expanded, setExpanded] = useState(shouldAutoExpand);
  const [viewingImage, setViewingImage] = useState<ImageAttachment | null>(null);

  const renderedContent = useMemo(() => {
    if (message.type === "assistant") {
      return <RichMarkdown text={message.content} fontScale={fontScale} />;
    }
    return null;
  }, [message.type, message.content, fontScale]);

  const handleImageClick = useCallback((att: ImageAttachment) => {
    setViewingImage(att);
  }, []);

  useEffect(() => {
    if (shouldAutoExpand) setExpanded(true);
  }, [shouldAutoExpand]);

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
              onClick={() => setExpanded(!expanded)}
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

    // codeterm channel (cross-pane, telegram, mesh) or wraps slash-commands.
    const originHeader = effectiveOrigin ? (() => {
      // Cross-pane / report: model-avatar + "from <Name>" + mesh·model subtitle.
      if (effectiveOrigin.kind === "pane") {
        const LeadIcon = effectiveOrigin.report ? CornerUpLeft : ArrowRight;
        const subtitle = [effectiveOrigin.mesh ? `via ${effectiveOrigin.mesh}` : null, effectiveOrigin.model]
          .filter(Boolean)
          .join(" · ");
        return (
          <button
            type="button"
            onClick={onOriginClick}
            disabled={!onOriginClick}
            className="flex items-center gap-1.5 mb-1"
            style={{
              cursor: onOriginClick ? "pointer" : "default",
              background: "transparent",
              border: "none",
              padding: 0,
              textAlign: "left",
            }}
            title={onOriginClick ? "Click to focus source pane" : undefined}
          >
            {originAvatar ?? <ModelAvatar model={effectiveOrigin.model} />}
            <span className="flex flex-col min-w-0" style={{ lineHeight: 1.1 }}>
              <span
                className="flex items-center gap-1 text-xs uppercase tracking-wider"
                style={{ color: "var(--ct-accent, #58a6ff)" }}
              >
                <LeadIcon size={11} style={{ opacity: 0.85 }} />
                <span className="truncate">
                  {effectiveOrigin.report ? "report · " : "from "}
                  {effectiveOrigin.title}
                </span>
                {inboundKind && (
                  <span
                    data-testid="chat-inbound-kind-badge"
                    className="rounded px-1 py-px text-[9px] font-mono uppercase"
                    style={{
                      background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 16%, transparent)",
                      color: "var(--ct-muted-fg, #8b949e)",
                      letterSpacing: 0,
                    }}
                  >
                    {INBOUND_KIND_LABELS[inboundKind]}
                  </span>
                )}
              </span>
              {subtitle && (
                <span
                  className="text-[10px] truncate"
                  style={{ color: "var(--ct-muted-fg, #8b949e)", textTransform: "none", letterSpacing: 0 }}
                >
                  {subtitle}
                </span>
              )}
            </span>
          </button>
        );
      }

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
          title={onOriginClick ? "Click to focus source pane" : undefined}
        >
          <OriginIcon size={12} />
          <span>{label}</span>
        </button>
      );
    })() : null;

    const userBubbleInner = (
      <>
        {effectiveOrigin?.kind === "command" ? (
          <CommandBody commands={effectiveOrigin.commands} body={message.content} />
        ) : (
          renderUserContent(message.content)
        )}
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

    if (effectiveOrigin?.kind === "pane") {
      return (
        <CrossPaneMessageShell header={originHeader} textSize={userText} footer={timestampFooter}>
          {userBubbleInner}
        </CrossPaneMessageShell>
      );
    }

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
    const label = message.tagName || message.systemSubtype || "system";
    const isCaveat = message.tagName === "local-command-caveat";
    const isSkillContext = message.systemSubtype === "skill_context";
    const isCompactSummary = message.systemSubtype === "compact_summary" || message.isCompactSummary === true;
    const isAgentInstructions = message.systemSubtype === "agent_instructions";
    const isTurnAborted = message.systemSubtype === "turn_aborted";
    if (isCompactSummary) {
      return (
        <LongArtifact
          label="Session continued from summary"
          text={message.content}
          expanded={expanded}
          onToggle={() => setExpanded(!expanded)}
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
    if (isAgentInstructions) {
      return (
        <LongArtifact
          label="System context"
          text={message.content}
          expanded={expanded}
          onToggle={() => setExpanded(!expanded)}
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
          onToggle={() => setExpanded(!expanded)}
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

  if (message.type === "tool_call") {
    const toolName = message.toolName || "";
    const filePath = extractFilePath(message);
    const command = extractCommand(message);
    const hasToolContent = message.toolInput?.content != null;
    const toolContent = hasToolContent ? String(message.toolInput!.content) : null;
    const edits = extractEdits(message);
    const addedLines = edits.reduce((n, e) => n + (e.new ? e.new.split("\n").length : 0), 0);
    const removedLines = edits.reduce((n, e) => n + (e.old ? e.old.split("\n").length : 0), 0);

    let summary = "";
    if (filePath) {
      summary = shortPath(filePath);
    } else if (command) {
      const firstLine = command.split("\n")[0];
      summary = firstLine.length > 60 ? firstLine.slice(0, 60) + "..." : firstLine;
    } else if (message.toolArgs) {
      const firstLine = message.toolArgs.split("\n")[0].trim();
      summary = firstLine.length > 60 ? firstLine.slice(0, 60) + "..." : firstLine;
    }

    if (message.toolName === "TodoWrite" && Array.isArray(message.toolInput?.todos)) {
      const todos = message.toolInput!.todos as Array<{ content: string; status: string; id?: string }>;
      const STATUS_ICON: Record<string, string> = { pending: "⏳", in_progress: "🚧", completed: "✅" };
      return (
        <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
          <div className="inline-flex max-w-full min-w-0 flex-col gap-0.5 rounded-md px-2.5 py-1.5" style={{ background: "var(--ct-surface)" }}>
            <span className="text-[10px] font-medium mb-0.5" style={{ color: "var(--ct-muted-fg)" }}>TodoWrite</span>
            {todos.map((t, i) => (
              <div key={t.id ?? i} className="flex items-start gap-1.5 text-xs" style={{ color: "var(--ct-fg)" }}>
                <span className="flex-shrink-0">{STATUS_ICON[t.status] ?? "⏳"}</span>
                <span className={t.status === "completed" ? "opacity-50 line-through" : ""}>{t.content}</span>
              </div>
            ))}
          </div>
        </div>
      );
    }

    // shell tool shapes.
    const shellCommand = command;
    if (shellCommand) {
      const inv = recognize(shellCommand);
      if (inv) {
        return (
          <CodetermCard
            inv={inv}
            result={message.toolResult}
            expanded={expanded}
            onToggle={() => setExpanded(!expanded)}
            resolvePaneName={resolvePaneName}
            onOpenPlan={onOpenPlan}
            fontScale={fontScale}
          />
        );
      }
    }

    const curatedKind = resolveCuratedToolKind(message.toolName);
    if (curatedKind) {
      const input = parsedToolInput(message);
      const Icon =
        curatedKind === "exec" ? Terminal :
        curatedKind === "read_file" ? FileText :
        curatedKind === "write_file" ? Pencil :
        curatedKind === "codeterm" ? Terminal :
        curatedKind === "mem_search" ? Search :
        Bot;
      const commandText =
        curatedKind === "exec"
          ? (textArg(input, ["cmd", "command"]) ?? extractCommand(message))
          : curatedKind === "codeterm" ? textArg(input, ["args", "command"]) :
        null;
      const pathText =
        curatedKind === "read_file" || curatedKind === "write_file"
          ? textArg(input, ["path", "file_path", "filePath"])
          : null;
      const queryText = curatedKind === "mem_search" ? textArg(input, ["query"]) : null;
      const providerText = curatedKind === "spawn_agent" ? textArg(input, ["provider"]) : null;
      const taskText = curatedKind === "spawn_agent" ? textArg(input, ["task"]) : null;
      const workspaceText = curatedKind === "spawn_agent" ? textArg(input, ["workspace"]) : null;
      const cwdText = curatedKind === "exec" ? textArg(input, ["cwd"]) : null;
      const contentText = curatedKind === "write_file" ? textArg(input, ["content"]) : null;
      const title =
        curatedKind === "exec" ? "shell" :
        curatedKind === "read_file" ? "read file" :
        curatedKind === "write_file" ? "write file" :
        curatedKind === "codeterm" ? "codeterm" :
        curatedKind === "mem_search" ? "mem search" :
        "spawn agent";
      const primary =
        commandText ?? pathText ?? queryText ?? taskText ?? message.toolArgs ?? message.content;
      const secondary =
        curatedKind === "exec" && cwdText ? `cwd ${cwdText}` :
        curatedKind === "write_file" && contentText ? `${contentSize(contentText)} | ${previewLine(contentText)}` :
        curatedKind === "spawn_agent"
          ? [providerText, workspaceText ? `workspace ${workspaceText}` : null].filter(Boolean).join(" | ")
          : null;

      return (
        <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
          <div className="flex w-full min-w-0 flex-col">
            <button
              data-testid={`chatbackend-tool-${curatedKind}`}
              onClick={() => setExpanded(!expanded)}
              className="flex w-full min-w-0 items-center gap-1.5 text-left rounded-md border px-2.5 py-1.5 text-xs transition-colors duration-150 hover:brightness-110"
              style={{
                background: "color-mix(in srgb, var(--ct-surface, #161b22) 88%, black 4%)",
                borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
                color: "var(--ct-muted-fg)",
              }}
            >
              <Icon
                size={13}
                strokeWidth={2}
                className="flex-shrink-0"
                style={{ color: "color-mix(in srgb, var(--ct-green, #3fb950) 75%, var(--ct-fg, #c9d1d9) 25%)" }}
              />
              <span className="font-medium font-mono" style={{ color: "var(--ct-muted-fg)" }}>
                {title}
              </span>
              <span className="min-w-0 flex-1 truncate font-mono text-[11px]" style={{ color: "var(--ct-fg)" }}>
                {primary}
              </span>
              {secondary && (
                <span className="hidden min-w-0 truncate opacity-45 font-mono text-[11px] sm:inline">
                  {secondary}
                </span>
              )}
              <ChevronIcon expanded={expanded} className="ml-1 opacity-50" />
            </button>

            {message.toolResult && !expanded && !isImageOnlyToolOutput(message.toolResult) && (
              <button
                onClick={() => setExpanded(true)}
                className="mt-0.5 flex min-w-0 items-start gap-1 pl-2.5 text-left text-[11px] font-mono"
                style={{ color: "var(--ct-muted-fg)" }}
                title="Show full output"
              >
                <span className="flex-shrink-0 opacity-40">{"→"}</span>
                <span className={`min-w-0 flex-1 ${TEXT_WRAP_CLASS} opacity-70`}>{toolResultPreviewLine(message.toolResult)}</span>
              </button>
            )}
            {message.toolResult && !expanded && isImageOnlyToolOutput(message.toolResult) && (
              <div className="mt-1 pl-2.5">
                <ToolOutputContent text={message.toolResult} />
              </div>
            )}

            {expanded && (
              <div
                className="mt-1 w-full min-w-0 max-w-full rounded-md border border-[var(--ct-border)] p-2 text-xs"
                style={{ background: "color-mix(in srgb, var(--ct-bg, #0d1117) 88%, black 4%)" }}
              >
                {curatedKind === "write_file" && contentText ? (
                  <CodeBlock text={contentText} language={languageFromPath(pathText)} maxHeight="24rem" />
                ) : (
                  <pre className={`${PRE_WRAP_CLASS} m-0 font-mono`} style={{ color: "var(--ct-fg)" }}>
                    {JSON.stringify(input, null, 2) || message.content}
                  </pre>
                )}
                {message.toolResult && (
                  <div className="mt-2">
                    <div
                      className="mb-1 text-[10px] font-medium uppercase tracking-wide"
                      style={{ color: "var(--ct-muted-fg)" }}
                    >
                      Result
                    </div>
                    <ToolOutputContent text={message.toolResult} maxHeight="12rem" />
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      );
    }

    const detailText = message.toolArgs || message.content;
    const codeLanguage = edits.length > 0
      ? languageForEdits(filePath, edits)
      : inferCodeLanguage(filePath, toolContent ?? "");
    const resultText = message.toolResult;

    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
        <div className="flex w-full min-w-0 flex-col">
          <button
            data-testid="chat-tool-call"
            onClick={() => setExpanded(!expanded)}
            className="flex w-full min-w-0 items-center gap-1.5 text-left rounded-md border px-2.5 py-1.5 text-xs transition-colors duration-150 hover:brightness-110"
            style={{
              background: "color-mix(in srgb, var(--ct-surface, #161b22) 88%, black 4%)",
              borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
              color: "var(--ct-muted-fg)",
            }}
          >
            <span
              className="flex-shrink-0"
              style={{ color: "color-mix(in srgb, var(--ct-green, #3fb950) 75%, var(--ct-fg, #c9d1d9) 25%)" }}
            >
              <ToolIcon name={toolName} />
            </span>
            <span className="font-medium font-mono shrink-0" style={{ color: "var(--ct-muted-fg)" }}>
              {toolDisplayLabel(toolName)}
            </span>
            {summary && (
              <span className="min-w-0 flex-1 truncate font-mono text-[11px]" style={{ color: "var(--ct-fg)" }}>{summary}</span>
            )}
            {toolContent && (
              <span className="shrink-0 opacity-40 font-mono text-[11px]">
                {lineCount(toolContent)} lines
              </span>
            )}
            {edits.length > 0 && (
              <span className="shrink-0 font-mono text-[11px]">
                <span style={{ color: "var(--ct-green, #3fb950)" }}>+{addedLines}</span>{" "}
                <span style={{ color: "var(--ct-red, #f85149)" }}>-{removedLines}</span>
              </span>
            )}
            <ChevronIcon expanded={expanded} className="ml-1 opacity-50" />
          </button>

          {/* Compact inline tail — last line of output, always visible while
              collapsed. Click to reveal the full result under the command. */}
          {resultText && !expanded && !isImageOnlyToolOutput(resultText) && (
            <button
              onClick={() => setExpanded(true)}
              className="mt-0.5 flex min-w-0 items-start gap-1 pl-2.5 text-left text-[11px] font-mono"
              style={{ color: "var(--ct-muted-fg)" }}
              title="Show full output"
            >
              <span className="flex-shrink-0 opacity-40">{"→"}</span>
              <span className={`min-w-0 flex-1 ${TEXT_WRAP_CLASS} opacity-70`}>{toolResultPreviewLine(resultText)}</span>
            </button>
          )}
          {resultText && !expanded && isImageOnlyToolOutput(resultText) && (
            <div className="mt-1 pl-2.5">
              <ToolOutputContent text={resultText} />
            </div>
          )}

          {expanded && (
            <div
              className="mt-1 w-full min-w-0 max-w-full rounded-md border border-[var(--ct-border)] p-2.5 text-xs"
              style={{ background: "color-mix(in srgb, var(--ct-bg, #0d1117) 88%, black 4%)" }}
            >
              {toolContent ? (
                <CodeBlock text={toolContent} language={codeLanguage} maxHeight="24rem" />
              ) : edits.length > 0 ? (
                <DiffView edits={edits} language={codeLanguage} />
              ) : (
                <pre
                  className={`${PRE_WRAP_CLASS} m-0 font-mono`}
                  style={{ color: "var(--ct-fg)" }}
                >
                  {detailText}
                </pre>
              )}
              {resultText && (
                <div className="mt-2">
                  <div
                    className="mb-1 text-[10px] font-medium uppercase tracking-wide"
                    style={{ color: "var(--ct-muted-fg)" }}
                  >
                    Result
                  </div>
                  <ToolOutputContent text={resultText} maxHeight="12rem" />
                </div>
              )}
            </div>
          )}
        </div>
      </div>
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
          onToggle={() => setExpanded(!expanded)}
        />
      );
    }
    const contentLines = message.content.split("\n");
    const preview = contentLines.slice(0, 2).join("\n");
    const hasMore = contentLines.length > 2;

    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto pl-4">
        <button
          onClick={() => setExpanded(!expanded)}
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
        <button
          onClick={() => setExpanded(!expanded)}
          className="inline-flex items-center gap-1.5 text-left rounded-md px-2.5 py-1 text-xs transition-colors duration-150 hover:brightness-110"
          style={{
            color: "var(--ct-muted-fg)",
          }}
        >
          <span className="opacity-40 text-[10px]">{"..."}</span>
          <span className="opacity-50 text-[11px]">Thinking</span>
          <ChevronIcon expanded={expanded} className="opacity-40" />
        </button>
        {expanded && (
          <div
            className="mt-1 max-w-full min-w-0 rounded-md px-3 py-2 text-xs overflow-x-auto max-h-48 overflow-y-auto italic border border-[var(--ct-border)]"
            style={{
              background: "var(--ct-bg)",
              color: "var(--ct-muted-fg)",
            }}
          >
            <pre className={`${PRE_WRAP_CLASS} m-0 font-[inherit]`}>
              {message.content}
            </pre>
          </div>
        )}
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
