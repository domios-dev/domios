import React, { useState, useMemo, useCallback, useEffect } from "react";
import type {
  ChatOrigin,
  NormalizedChatMessage as ChatMessage,
  ChatAttachment as ImageAttachment,
} from "@codeterm/chat-core";
import {
  Bot,
  CheckCircle2,
  Clock,
  Download, FileText, Image,
  Pencil,
  Search,
  Terminal,
  // Origin icons
  ArrowRight,      // pane -> pane
  Send,            // generic chat (telegram / service)
  Network,         // mesh
  ChevronsRight,   // slash-command chip
  CornerUpLeft,    // report (bubbled up to parent)
  Settings,        // plugin_config (Configure card)
  Info,
} from "lucide-react";
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
  PlainOutput,
  RichMarkdown,
  TEXT_WRAP_CLASS,
  extractCommand,
  extractEdits,
  extractFilePath,
  isLargeArtifact,
  languageFromPath,
  lastNonEmptyLine,
  renderUserContent,
  shortPath,
} from "./MarkdownBody";
import { ToolIcon } from "./toolIcons";
import { InteractiveElementRenderer } from "./interactive/InteractiveElementRenderer";
import type { InteractiveAnswerHandler } from "./interactive/InteractiveQuestion";
import { pickUiFontScaleValue } from "./uiFontScale";

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
  fontScale?: number;
}

// Kill switch for interactive AskUserQuestion widgets (all providers). Detection
// is mistimed — the prompt surfaces AFTER the user has already answered — so the
// widget is unreliable. Disabled until the timing is fixed; the question then
// renders as a plain tool-call pill instead.
const INTERACTIVE_QUESTIONS_ENABLED = false;

type CuratedToolKind = "exec" | "read_file" | "write_file" | "codeterm" | "mem_search" | "spawn_agent";
type StructuredInboundKind = NonNullable<ChatMessage["inboundKind"]>;

const INBOUND_KIND_LABELS: Record<StructuredInboundKind, string> = {
  pane: "pane",
  report: "report",
  reminder: "reminder",
  mesh: "mesh",
  telegram: "telegram",
};

interface StructuredInbound {
  origin: ChatOrigin;
  kind: StructuredInboundKind;
}

function structuredInboundFromMessage(message: ChatMessage): StructuredInbound | null {
  const fromLabel = message.fromLabel?.trim();
  if (!fromLabel) return null;
  const kind = message.inboundKind ?? (message.fromMesh ? "mesh" : "pane");
  const title = kind === "mesh" && message.fromMesh
    ? `${fromLabel}@${message.fromMesh}`
    : fromLabel;
  return {
    kind,
    origin: {
      kind: "pane",
      title,
      paneIdShort: message.fromPane ?? "",
      model: message.fromModel,
      mesh: kind === "mesh" ? undefined : message.fromMesh,
      report: kind === "report",
    },
  };
}

function parsedToolInput(message: ChatMessage): Record<string, unknown> {
  if (message.toolInput) return message.toolInput;
  if (!message.toolArgs) return {};
  try {
    const parsed = JSON.parse(message.toolArgs);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? parsed as Record<string, unknown>
      : {};
  } catch {
    return {};
  }
}

function textArg(input: Record<string, unknown>, keys: string[]): string | null {
  for (const key of keys) {
    const value = input[key];
    if (typeof value === "string" && value.trim()) return value;
    if (Array.isArray(value) && value.length > 0) return value.map(String).join(" ");
  }
  return null;
}

function contentSize(content: string): string {
  const bytes = new TextEncoder().encode(content).length;
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}

function previewLine(text: string, max = 90): string {
  const line = text.replace(/\r/g, "").split("\n").find((part) => part.trim())?.trim() ?? "";
  return line.length > max ? `${line.slice(0, max)}...` : line;
}

function curatedToolKind(name: string | undefined): CuratedToolKind | null {
  const normalized = (name ?? "").trim().toLowerCase();
  switch (normalized) {
    case "exec":
    case "read_file":
    case "write_file":
    case "codeterm":
    case "mem_search":
    case "spawn_agent":
      return normalized;
    default:
      return null;
  }
}

function hasRenderableValue(value: unknown): boolean {
  if (value == null) return false;
  if (typeof value === "string") return value.trim().length > 0;
  if (Array.isArray(value)) return value.length > 0;
  if (typeof value === "object") return Object.keys(value).length > 0;
  return true;
}

function hasRenderableText(value: string | undefined): boolean {
  return typeof value === "string" && value.trim().length > 0;
}

function isRenderableMessage(message: ChatMessage): boolean {
  if (message.type === "tool_call") {
    return (
      hasRenderableText(message.content) ||
      hasRenderableText(message.toolName) ||
      hasRenderableText(message.toolArgs) ||
      hasRenderableText(message.toolResult) ||
      hasRenderableValue(message.toolInput) ||
      Boolean(message.interactive?.length)
    );
  }

  if (message.type === "tool_result") {
    return hasRenderableText(message.content) || hasRenderableText(message.toolResult);
  }

  return true;
}

export const MessageBubble = React.memo(function MessageBubble({ message, origin, originAvatar, onOriginClick, resolvePaneName, onAnswerInteractive, answeredInteractiveIds, fontScale }: MessageBubbleProps) {
  // Auto-expand for Write-style tool input (the file body being written) and
  // for Edit/MultiEdit (always show the diff). A bash/grep result must NOT
  // force-expand — it shows a compact inline tail under the pill instead.
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

  // Image message (standalone, legacy) — compact placeholder
  if (message.type === "image") {
    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
        <ImageBadge />
      </div>
    );
  }

  // User message — right-aligned by default; left-aligned with origin header for injected messages
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

    // Origin header rendered above the bubble when the message came from a
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

    return (
      <div className={`group flex mb-3 w-full max-w-4xl min-w-0 mx-auto ${effectiveOrigin ? "justify-start" : "justify-end"}`}>
        <div className="min-w-0 max-w-[75%] sm:max-w-[75%]">
          {originHeader}
          <div
            className={`min-w-0 rounded-2xl rounded-br-sm px-4 py-2 ${userText} select-text cursor-text border ${TEXT_WRAP_CLASS}`}
            style={{
              // The user voice is the ONE filled element in the feed — a clear accent
              // tint so it never blends with tool traces or assistant prose.
              background: effectiveOrigin
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, var(--ct-surface, #161b22) 88%)"
                : "color-mix(in srgb, var(--ct-accent, #58a6ff) 22%, var(--ct-bg, #0d1117) 78%)",
              borderColor: effectiveOrigin
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)"
                : "color-mix(in srgb, var(--ct-accent, #58a6ff) 45%, transparent)",
              color: "var(--ct-fg)",
            }}
          >
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
            {/* Inline image preview */}
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
          </div>
          {(() => {
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
          })()}
        </div>
      </div>
    );
  }

  // Assistant message — left-aligned, NO bubble, clean prose
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

  // Interactive prompt — answerable widget rendered in place of the raw
  // tool-call JSON (e.g. AskUserQuestion). Provider-agnostic: the backend
  // already parsed the tool payload into `interactive`.
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

  // Tool call — compact pill/card
  if (message.type === "tool_call") {
    const toolName = message.toolName || "";
    const filePath = extractFilePath(message);
    const command = extractCommand(message);
    const hasToolContent = message.toolInput?.content != null;
    const toolContent = hasToolContent ? String(message.toolInput!.content) : null;
    const edits = extractEdits(message);
    const addedLines = edits.reduce((n, e) => n + (e.new ? e.new.split("\n").length : 0), 0);
    const removedLines = edits.reduce((n, e) => n + (e.old ? e.old.split("\n").length : 0), 0);

    // Build summary text
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

    // TodoWrite — render checklist inline
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

    // CodeTerm CLI command — render a branded semantic card across Claude/Codex
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
          />
        );
      }
    }

    const curatedKind = curatedToolKind(message.toolName);
    if (curatedKind) {
      const input = parsedToolInput(message);
      const phase = message.toolResult ? "result" : message.toolId ? "running" : "pending";
      const Icon =
        curatedKind === "exec" ? Terminal :
        curatedKind === "read_file" ? FileText :
        curatedKind === "write_file" ? Pencil :
        curatedKind === "codeterm" ? Terminal :
        curatedKind === "mem_search" ? Search :
        Bot;
      const PhaseIcon = phase === "result" ? CheckCircle2 : phase === "running" ? Clock : Clock;
      const commandText =
        curatedKind === "exec" ? textArg(input, ["cmd", "command"]) :
        curatedKind === "codeterm" ? textArg(input, ["args", "command"]) :
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
        curatedKind === "exec" ? "exec" :
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
          <div className="inline-flex flex-col max-w-[min(52rem,100%)]">
            <button
              data-testid={`chatbackend-tool-${curatedKind}`}
              onClick={() => setExpanded(!expanded)}
              className="inline-flex min-w-0 items-center gap-1.5 text-left rounded-md border px-2.5 py-1 text-xs transition-colors duration-150 hover:brightness-110"
              style={{
                background: "transparent",
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
              <span className="min-w-0 truncate opacity-70 font-mono text-[11px]">
                {primary}
              </span>
              {secondary && (
                <span className="hidden min-w-0 truncate opacity-45 font-mono text-[11px] sm:inline">
                  {secondary}
                </span>
              )}
              <span
                className="ml-1 inline-flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-mono"
                style={{
                  background: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 10%, transparent)",
                  color: phase === "result" ? "var(--ct-green, #3fb950)" : "var(--ct-muted-fg)",
                }}
              >
                <PhaseIcon size={10} strokeWidth={2} />
                {phase}
              </span>
              <ChevronIcon expanded={expanded} className="ml-1 opacity-50" />
            </button>

            {message.toolResult && !expanded && (
              <button
                onClick={() => setExpanded(true)}
                className="mt-0.5 flex min-w-0 items-start gap-1 pl-2.5 text-left text-[11px] font-mono"
                style={{ color: "var(--ct-muted-fg)" }}
                title="Show full output"
              >
                <span className="flex-shrink-0 opacity-40">{"->"}</span>
                <span className="min-w-0 truncate opacity-70">{lastNonEmptyLine(message.toolResult)}</span>
              </button>
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
                    <PlainOutput text={message.toolResult} maxHeight="12rem" />
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      );
    }

    const detailText = message.toolArgs || message.content;
    const codeLanguage = languageFromPath(filePath);
    const resultText = message.toolResult;

    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
        <div className="inline-flex flex-col max-w-[min(52rem,100%)]">
          <button
            data-testid="chat-tool-call"
            onClick={() => setExpanded(!expanded)}
            // De-bubbled on purpose: tool activity is a thin terminal trace (hairline border,
            // no fill, green exec glyph) so it visually recedes behind user/assistant voices.
            className="inline-flex min-w-0 items-center gap-1.5 text-left rounded-md border px-2.5 py-1 text-xs transition-colors duration-150 hover:brightness-110"
            style={{
              background: "transparent",
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
            <span className="font-medium font-mono" style={{ color: "var(--ct-muted-fg)" }}>
              {toolName === "shell_command" ? "shell" : message.toolName}
            </span>
            {summary && (
              <span className="min-w-0 truncate opacity-60 font-mono text-[11px]">{summary}</span>
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
          {resultText && !expanded && (
            <button
              onClick={() => setExpanded(true)}
              className="mt-0.5 flex min-w-0 items-start gap-1 pl-2.5 text-left text-[11px] font-mono"
              style={{ color: "var(--ct-muted-fg)" }}
              title="Show full output"
            >
              <span className="flex-shrink-0 opacity-40">{"→"}</span>
              <span className="min-w-0 truncate opacity-70">{lastNonEmptyLine(resultText)}</span>
            </button>
          )}

          {expanded && (
            <div
              className="mt-1 w-full min-w-0 max-w-full rounded-md border border-[var(--ct-border)] p-2 text-xs"
              style={{ background: "color-mix(in srgb, var(--ct-bg, #0d1117) 88%, black 4%)" }}
            >
              {toolContent ? (
                <CodeBlock text={toolContent} language={codeLanguage} maxHeight="24rem" />
              ) : edits.length > 0 ? (
                <DiffView edits={edits} />
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
                  <PlainOutput text={resultText} maxHeight="12rem" />
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    );
  }

  // Tool result — subtle, compact
  if (message.type === "tool_result") {
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
          <div
            className="mt-1 ml-4 max-w-full min-w-0 rounded-md px-3 py-2 text-xs font-mono overflow-x-auto max-h-48 overflow-y-auto border border-[var(--ct-border)]"
            style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}
          >
            <pre className={`${PRE_WRAP_CLASS} m-0`}>{message.content}</pre>
          </div>
        )}
      </div>
    );
  }

  // Thinking block — minimal, collapsed by default
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

  // Status message
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
