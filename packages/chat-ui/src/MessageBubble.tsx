import React, { useState, useMemo, useCallback, useEffect } from "react";
import type {
  ChatOrigin,
  NormalizedChatMessage as ChatMessage,
  ChatAttachment as ImageAttachment,
} from "@codeterm/chat-core";
import {
  Download, Image,
  // Origin icons
  ArrowRight,      // pane -> pane
  Send,            // generic chat (telegram / service)
  Network,         // mesh
  ChevronsRight,   // slash-command chip
  CornerUpLeft,    // report (bubbled up to parent)
  Info,
} from "lucide-react";
import { recognize } from "./codeterm/recognize";
import { CodetermCard } from "./codeterm/CodetermCard";
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

interface MessageBubbleProps {
  message: ChatMessage;
  /** When set, this user message arrived via a codeterm channel (cross-pane,
   *  telegram, mesh, slash-command) and should render as a typed header
   *  instead of the default right-aligned "user" styling. */
  origin?: ChatOrigin;
  originAvatar?: React.ReactNode;
  /** When set, clicking the origin header focuses the source pane. */
  onOriginClick?: () => void;
  /** Resolve a pane id/short-id to a tab name for CodeTerm cards (e.g. "Sent to <name>"). */
  resolvePaneName?: PaneNameResolver;
}

export const MessageBubble = React.memo(function MessageBubble({ message, origin, originAvatar, onOriginClick, resolvePaneName }: MessageBubbleProps) {
  // Auto-expand for Write-style tool input (the file body being written) and
  // for Edit/MultiEdit (always show the diff). A bash/grep result must NOT
  // force-expand — it shows a compact inline tail under the pill instead.
  const shouldAutoExpand = message.type === "tool_call"
    && (message.toolInput?.content != null || extractEdits(message).length > 0);
  const [expanded, setExpanded] = useState(shouldAutoExpand);
  const [viewingImage, setViewingImage] = useState<ImageAttachment | null>(null);

  const renderedContent = useMemo(() => {
    if (message.type === "assistant") {
      return <RichMarkdown text={message.content} />;
    }
    return null;
  }, [message.type, message.content]);

  const handleImageClick = useCallback((att: ImageAttachment) => {
    setViewingImage(att);
  }, []);

  useEffect(() => {
    if (shouldAutoExpand) setExpanded(true);
  }, [shouldAutoExpand]);

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

    // Origin header rendered above the bubble when the message came from a
    // codeterm channel (cross-pane, telegram, mesh) or wraps slash-commands.
    const originHeader = origin ? (() => {
      // Cross-pane / report: model-avatar + "from <Name>" + mesh·model subtitle.
      if (origin.kind === "pane") {
        const LeadIcon = origin.report ? CornerUpLeft : ArrowRight;
        const subtitle = [origin.mesh ? `via ${origin.mesh}` : null, origin.model]
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
            {originAvatar ?? <ModelAvatar model={origin.model} />}
            <span className="flex flex-col min-w-0" style={{ lineHeight: 1.1 }}>
              <span
                className="flex items-center gap-1 text-xs uppercase tracking-wider"
                style={{ color: "var(--ct-accent, #58a6ff)" }}
              >
                <LeadIcon size={11} style={{ opacity: 0.85 }} />
                <span className="truncate">
                  {origin.report ? "report · " : "from "}
                  {origin.title}
                </span>
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
        origin.kind === "mesh"    ? Network :
        origin.kind === "command" ? ChevronsRight :
        Send;
      const label =
        origin.kind === "telegram"? `Telegram · ${origin.userId.slice(0, 12)}` :
        origin.kind === "mesh"    ? `Mesh · ${origin.peerId.slice(0, 10)}` :
        origin.kind === "command" ? (
          origin.commands.length === 1
            ? origin.commands[0].name
            : `${origin.commands.length} commands`
        ) :
        `${origin.service} · ${origin.userId.slice(0, 12)}`;
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
      <div className={`flex mb-3 w-full max-w-4xl min-w-0 mx-auto ${origin ? "justify-start" : "justify-end"}`}>
        <div className="min-w-0 max-w-[75%] sm:max-w-[75%]">
          {originHeader}
          <div
            className={`min-w-0 rounded-2xl rounded-br-sm px-4 py-2 text-sm select-text cursor-text border ${TEXT_WRAP_CLASS}`}
            style={{
              // The user voice is the ONE filled element in the feed — a clear accent
              // tint so it never blends with tool traces or assistant prose.
              background: origin
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, var(--ct-surface, #161b22) 88%)"
                : "color-mix(in srgb, var(--ct-accent, #58a6ff) 22%, var(--ct-bg, #0d1117) 78%)",
              borderColor: origin
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)"
                : "color-mix(in srgb, var(--ct-accent, #58a6ff) 45%, transparent)",
              color: "var(--ct-fg)",
            }}
          >
            {origin?.kind === "command" ? (
              <CommandBody commands={origin.commands} body={message.content} />
            ) : (
              renderUserContent(message.content)
            )}
            {attachments && attachments.length > 0 && (
              <div className="flex flex-wrap gap-1.5 mt-2 pt-2" style={{ borderTop: "1px solid color-mix(in srgb, var(--ct-accent) 15%, transparent)" }}>
                {attachments.map((att) => (
                  <button
                    key={att.id}
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
              <div className="mt-2 pt-2" style={{ borderTop: "1px solid color-mix(in srgb, var(--ct-accent) 15%, transparent)" }}>
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
        </div>
      </div>
    );
  }

  // Assistant message — left-aligned, NO bubble, clean prose
  if (message.type === "assistant") {
    return (
      <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto">
        <div
          className={`min-w-0 max-w-full text-sm leading-relaxed select-text cursor-text ${TEXT_WRAP_CLASS}`}
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

    const detailText = message.toolArgs || message.content;
    const codeLanguage = languageFromPath(filePath);
    const resultText = message.toolResult;

    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
        <div className="inline-flex flex-col max-w-[min(52rem,100%)]">
          <button
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
