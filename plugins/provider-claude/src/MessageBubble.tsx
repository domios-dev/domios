import React, { useState, useMemo, useCallback } from "react";
import type { ChatMessage, ImageAttachment } from "./parser";
import { ChevronDown, File, Image, Download } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";

const TOOL_ICONS: Record<string, string> = {
  Bash: "\u26A1",
  Read: "\uD83D\uDCC4",
  Write: "\u270F\uFE0F",
  Edit: "\uD83D\uDD27",
  Agent: "\uD83E\uDD16",
  Grep: "\uD83D\uDD0D",
  Glob: "\uD83D\uDCC2",
  Skill: "\u2B50",
  Update: "\uD83D\uDD04",
  ToolSearch: "\uD83D\uDD0D",
};

/**
 * Real markdown renderer for assistant text.
 * - GFM tables, task lists, strikethrough via remark-gfm
 * - Syntax-highlighted fenced code blocks via react-syntax-highlighter
 * - All inline elements themed with codeterm CSS vars
 *
 * Note: react-markdown v10 no longer passes `inline` to the code component.
 * Block code is detected by the presence of a `language-*` className (set by
 * remark on fenced blocks). Inline <code> never has that className.
 */
function RichMarkdown({ text }: { text: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        h1: ({ node: _node, ...props }) => (
          <h1 className="text-base font-bold mt-3 mb-1" style={{ color: "var(--ct-fg)" }} {...props} />
        ),
        h2: ({ node: _node, ...props }) => (
          <h2 className="text-sm font-semibold mt-2 mb-1" style={{ color: "var(--ct-fg)" }} {...props} />
        ),
        h3: ({ node: _node, ...props }) => (
          <h3 className="text-sm font-medium mt-1.5 mb-0.5" style={{ color: "var(--ct-fg)" }} {...props} />
        ),
        p: ({ node: _node, ...props }) => <p className="my-1" {...props} />,
        ul: ({ node: _node, ...props }) => <ul className="list-disc ml-5 my-1" {...props} />,
        ol: ({ node: _node, ...props }) => <ol className="list-decimal ml-5 my-1" {...props} />,
        li: ({ node: _node, children, ...props }) => (
          <li className="my-0.5" {...props}>{children}</li>
        ),
        table: ({ node: _node, ...props }) => (
          <div className="overflow-x-auto my-2">
            <table
              className="text-xs border-collapse"
              style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 30%, transparent)" }}
              {...props}
            />
          </div>
        ),
        thead: ({ node: _node, ...props }) => (
          <thead style={{ background: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 8%, transparent)" }} {...props} />
        ),
        th: ({ node: _node, ...props }) => (
          <th
            className="px-2 py-1 text-left font-semibold border"
            style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 30%, transparent)" }}
            {...props}
          />
        ),
        td: ({ node: _node, ...props }) => (
          <td
            className="px-2 py-1 border align-top"
            style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 20%, transparent)" }}
            {...props}
          />
        ),
        // Block code: SyntaxHighlighter. Inline code: styled span.
        // Detection: fenced blocks always get a "language-*" className from remark.
        code: ({ node: _node, className, children, ...props }: React.ComponentProps<"code"> & { node?: unknown }) => {
          const match = /language-(\w+)/.exec(className || "");
          if (!match) {
            return (
              <code
                className="px-1 py-0.5 rounded text-xs font-mono"
                style={{
                  background: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 18%, transparent)",
                  color: "var(--ct-fg, #c9d1d9)",
                }}
                {...props}
              >
                {children}
              </code>
            );
          }
          return (
            <SyntaxHighlighter
              language={match[1]}
              style={vscDarkPlus}
              PreTag="div"
              customStyle={{
                margin: "0.5rem 0",
                borderRadius: "0.375rem",
                padding: "0.75rem",
                fontSize: "0.75rem",
                lineHeight: "1.4",
                background: "color-mix(in srgb, var(--ct-bg, #0d1117) 80%, black 5%)",
              }}
              codeTagProps={{ style: { fontFamily: "inherit" } }}
            >
              {String(children).replace(/\n$/, "")}
            </SyntaxHighlighter>
          );
        },
        // Strip default <pre> wrapper — SyntaxHighlighter renders its own via PreTag
        pre: ({ node: _node, children }) => <>{children}</>,
        a: ({ node: _node, ...props }) => (
          <a
            target="_blank"
            rel="noopener noreferrer"
            className="underline"
            style={{ color: "var(--ct-accent, #58a6ff)" }}
            {...props}
          />
        ),
        del: ({ node: _node, ...props }) => <del className="opacity-60" {...props} />,
        blockquote: ({ node: _node, ...props }) => (
          <blockquote
            className="border-l-2 pl-3 my-2 italic opacity-80"
            style={{ borderColor: "var(--ct-muted-fg, #8b949e)" }}
            {...props}
          />
        ),
        hr: () => (
          <hr
            className="my-2"
            style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 30%, transparent)" }}
          />
        ),
      }}
    >
      {text}
    </ReactMarkdown>
  );
}

/** Extract file path from tool args or toolInput. */
function extractFilePath(message: ChatMessage): string | null {
  if (message.toolInput) {
    const fp = message.toolInput.file_path as string | undefined;
    if (fp) return fp;
  }
  if (message.toolArgs) {
    try {
      const parsed = JSON.parse(message.toolArgs);
      if (parsed.file_path) return parsed.file_path;
    } catch {
      // Try to extract from text
      const match = message.toolArgs.match(/(?:file_path|path)['":\s]+([^\s'"}\],]+)/);
      if (match) return match[1];
    }
  }
  return null;
}

/** Truncate file path to show just filename + parent. */
function shortPath(fullPath: string): string {
  const parts = fullPath.split("/").filter(Boolean);
  if (parts.length <= 2) return fullPath;
  return ".../" + parts.slice(-2).join("/");
}

/** Extract command from Bash tool args. */
function extractCommand(message: ChatMessage): string | null {
  if (message.toolInput) {
    const cmd = message.toolInput.command as string | undefined;
    if (cmd) return cmd;
  }
  if (message.toolArgs) {
    try {
      const parsed = JSON.parse(message.toolArgs);
      if (parsed.command) return parsed.command;
    } catch {
      // fallback
    }
  }
  return null;
}

/** Chevron icon for collapsible sections. */
function ChevronIcon({ expanded, className = "" }: { expanded: boolean; className?: string }) {
  return (
    <ChevronDown
      className={`w-3 h-3 flex-shrink-0 transition-transform duration-150 ${expanded ? "rotate-180" : ""} ${className}`}
      strokeWidth={1.5}
    />
  );
}

/** File icon SVG for attachment badges. */
function FileIcon({ className = "" }: { className?: string }) {
  return <File className={className} size={12} strokeWidth={2} />;
}

/** Extract filename from path. */
function fileName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

/**
 * Render user message content, replacing file attachment markers with badges.
 * Marker format: \x00«file:/path/to/file» — uses NUL + guillemets to avoid
 * false positives with any user-typed text, markdown, or code.
 */
function renderUserContent(content: string): React.ReactNode {
  // Match both new marker (\x00«file:...») and legacy [attached: ...] format
  const attachRe = /(?:\x00\u00ABfile:([^\u00BB]+)\u00BB|\[attached:\s*([^\]]+)\])/g;
  const parts: React.ReactNode[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  let key = 0;

  while ((match = attachRe.exec(content)) !== null) {
    // Text before the match
    const before = content.slice(lastIndex, match.index).trim();
    if (before) {
      parts.push(<pre key={key++} className="whitespace-pre-wrap font-[inherit] m-0 select-text">{before}</pre>);
    }
    // File badge — group 1 = new marker, group 2 = legacy format
    const filePath = (match[1] || match[2]).trim();
    const name = fileName(filePath);
    const ext = name.split(".").pop()?.toUpperCase() || "FILE";
    parts.push(
      <div
        key={key++}
        className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 mt-1 text-[11px]"
        style={{
          background: "color-mix(in srgb, var(--ct-accent) 15%, transparent)",
          color: "var(--ct-muted-fg)",
        }}
        title={filePath}
      >
        <FileIcon />
        <span className="font-medium">{name}</span>
        <span className="opacity-50">{ext}</span>
      </div>,
    );
    lastIndex = match.index + match[0].length;
  }

  // Remaining text
  const rest = content.slice(lastIndex).trim();
  if (rest) {
    parts.push(<pre key={key++} className="whitespace-pre-wrap font-[inherit] m-0 select-text">{rest}</pre>);
  }

  // No attachments found — plain text
  if (parts.length === 0) {
    return <pre className="whitespace-pre-wrap font-[inherit] m-0 select-text">{content}</pre>;
  }

  return <>{parts}</>;
}

/** Compact image indicator for standalone image messages. */
function ImageBadge() {
  return (
    <span
      className="inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs"
      style={{ background: "var(--ct-surface)", color: "var(--ct-muted-fg)" }}
    >
      <Image size={14} strokeWidth={2} />
      <span className="opacity-70">Image</span>
    </span>
  );
}

export const MessageBubble = React.memo(function MessageBubble({ message }: { message: ChatMessage }) {
  const [expanded, setExpanded] = useState(false);
  const [codeExpanded, setCodeExpanded] = useState(false);
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

  // Image message (standalone, legacy) — compact placeholder
  if (message.type === "image") {
    return (
      <div className="mb-1.5 max-w-4xl mx-auto">
        <ImageBadge />
      </div>
    );
  }

  // User message — right-aligned, subtle accent bubble
  if (message.type === "user") {
    const attachments = message.attachments;
    return (
      <div className="flex justify-end mb-3 max-w-4xl mx-auto">
        <div
          className="max-w-[75%] rounded-2xl rounded-br-sm px-4 py-2 text-sm select-text cursor-text border"
          style={{
            background: "color-mix(in srgb, var(--ct-accent) 10%, transparent)",
            borderColor: "color-mix(in srgb, var(--ct-accent) 20%, transparent)",
            color: "var(--ct-fg)",
          }}
        >
          {renderUserContent(message.content)}
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
    );
  }

  // Assistant message — left-aligned, NO bubble, clean prose
  if (message.type === "assistant") {
    return (
      <div className="mb-3 max-w-4xl mx-auto">
        <div
          className="text-sm leading-relaxed select-text cursor-text"
          style={{ color: "var(--ct-fg)" }}
        >
          {renderedContent}
        </div>
      </div>
    );
  }

  // Tool call — compact pill/card
  if (message.type === "tool_call") {
    const icon = TOOL_ICONS[message.toolName || ""] || "\u2699\uFE0F";
    const filePath = extractFilePath(message);
    const command = message.toolName === "Bash" ? extractCommand(message) : null;
    const hasToolContent = message.toolInput?.content != null;
    const toolContent = hasToolContent ? String(message.toolInput!.content) : null;

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
        <div className="mb-1.5 max-w-4xl mx-auto">
          <div className="inline-flex flex-col gap-0.5 rounded-md px-2.5 py-1.5" style={{ background: "var(--ct-surface)" }}>
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

    return (
      <div className="mb-1.5 max-w-4xl mx-auto">
        <div className="inline-flex flex-col max-w-md">
          {/* Main tool pill */}
          <button
            onClick={() => setExpanded(!expanded)}
            className="inline-flex items-center gap-1.5 text-left rounded-md px-2.5 py-1 text-xs transition-colors duration-150 hover:brightness-110"
            style={{
              background: "var(--ct-surface)",
              color: "var(--ct-muted-fg)",
            }}
          >
            <span className="text-xs flex-shrink-0">{icon}</span>
            <span className="font-medium" style={{ color: "var(--ct-fg)" }}>{message.toolName}</span>
            {summary && (
              <span className="truncate opacity-60 font-mono text-[11px]">{summary}</span>
            )}
            <ChevronIcon expanded={expanded} className="ml-1 opacity-50" />
          </button>

          {/* Expanded: full tool args */}
          {expanded && (
            <div
              className="mt-1 rounded-md px-3 py-2 text-xs font-mono overflow-x-auto max-h-48 overflow-y-auto border border-[var(--ct-border)]"
              style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}
            >
              <pre className="whitespace-pre-wrap m-0">{message.toolArgs || message.content}</pre>
            </div>
          )}

          {/* Write/Edit content: collapsible code block */}
          {toolContent && (
            <div className="mt-1">
              <button
                onClick={() => setCodeExpanded(!codeExpanded)}
                className="inline-flex items-center gap-1 text-[11px] px-2 py-0.5 rounded transition-colors duration-150 hover:brightness-110"
                style={{
                  color: "var(--ct-muted-fg)",
                  background: "var(--ct-surface)",
                }}
              >
                <span className="font-mono opacity-70">
                  {codeExpanded ? "Hide" : "Show"} content
                </span>
                <span className="opacity-40 font-mono">
                  ({toolContent.split("\n").length} lines)
                </span>
                <ChevronIcon expanded={codeExpanded} className="opacity-40" />
              </button>
              {codeExpanded && (
                <div
                  className="mt-1 rounded-md overflow-x-auto max-h-72 overflow-y-auto border border-[var(--ct-border)]"
                  style={{ background: "var(--ct-bg)" }}
                >
                  <pre className="px-3 py-2 text-xs font-mono whitespace-pre-wrap m-0" style={{ color: "var(--ct-fg)" }}>
                    <code>{toolContent}</code>
                  </pre>
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
    const contentLines = message.content.split("\n");
    const preview = contentLines.slice(0, 2).join("\n");
    const hasMore = contentLines.length > 2;

    return (
      <div className="mb-1.5 max-w-4xl mx-auto pl-4">
        <button
          onClick={() => setExpanded(!expanded)}
          className="inline-flex items-start gap-1.5 text-left rounded-md px-2.5 py-1 text-xs max-w-md transition-colors duration-150 hover:brightness-110"
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
            className="mt-1 ml-4 rounded-md px-3 py-2 text-xs font-mono overflow-x-auto max-h-48 overflow-y-auto border border-[var(--ct-border)]"
            style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}
          >
            <pre className="whitespace-pre-wrap m-0">{message.content}</pre>
          </div>
        )}
      </div>
    );
  }

  // Thinking block — minimal, collapsed by default
  if (message.type === "thinking") {
    return (
      <div className="mb-1.5 max-w-4xl mx-auto">
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
            className="mt-1 rounded-md px-3 py-2 text-xs overflow-x-auto max-h-48 overflow-y-auto italic border border-[var(--ct-border)]"
            style={{
              background: "var(--ct-bg)",
              color: "var(--ct-muted-fg)",
            }}
          >
            <pre className="whitespace-pre-wrap m-0 font-[inherit]">
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
      <div className="flex justify-center mb-2 max-w-4xl mx-auto">
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
