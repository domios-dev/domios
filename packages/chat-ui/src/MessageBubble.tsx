import React, { useState, useMemo, useCallback, useEffect } from "react";
import type {
  ChatOrigin,
  NormalizedChatMessage as ChatMessage,
  ChatAttachment as ImageAttachment,
  SlashCommand,
} from "@codeterm/chat-core";
import {
  ChevronDown, File, Image, Download,
  // Tool icons
  Terminal,        // Bash
  FileText,        // Read
  Pencil,          // Write
  Wrench,          // Edit
  Bot,             // Agent / Task
  Search,          // Grep, ToolSearch
  Folder,          // Glob
  Sparkles,        // Skill
  RefreshCw,       // Update
  // Origin icons
  ArrowRight,      // pane -> pane
  Send,            // generic chat (telegram / service)
  Network,         // mesh
  ChevronsRight,   // slash-command chip
  Info,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import * as prismStyles from "react-syntax-highlighter/dist/esm/styles/prism";

const vscDarkPlus =
  ((prismStyles as { vscDarkPlus?: Record<string, React.CSSProperties> }).vscDarkPlus ??
    (prismStyles as { default?: { vscDarkPlus?: Record<string, React.CSSProperties> } }).default?.vscDarkPlus);

const TOOL_ICONS: Record<string, LucideIcon> = {
  Bash: Terminal,
  Read: FileText,
  Write: Pencil,
  Edit: Wrench,
  Agent: Bot,
  Task: Bot,
  Grep: Search,
  Glob: Folder,
  Skill: Sparkles,
  Update: RefreshCw,
  ToolSearch: Search,
};

const PRE_WRAP_CLASS = "whitespace-pre-wrap break-words [overflow-wrap:anywhere]";
const TEXT_WRAP_CLASS = "break-words [overflow-wrap:anywhere]";

function ToolIcon({ name }: { name: string }) {
  const Icon = TOOL_ICONS[name] ?? Wrench;
  return <Icon size={12} className="inline opacity-70" style={{ verticalAlign: "-2px" }} />;
}

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
        p: ({ node: _node, ...props }) => <p className={`my-1 ${TEXT_WRAP_CLASS}`} {...props} />,
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
                  overflowWrap: "anywhere",
                  wordBreak: "break-word",
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
                maxWidth: "100%",
                overflowWrap: "anywhere",
                wordBreak: "break-word",
              }}
              codeTagProps={{
                style: {
                  fontFamily: "inherit",
                  overflowWrap: "anywhere",
                  wordBreak: "break-word",
                },
              }}
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
  const parts = fullPath.split(/[\\/]/).filter(Boolean);
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

function languageFromPath(path: string | null): string {
  if (!path) return "text";
  const ext = path.split(/[./\\]/).pop()?.toLowerCase();
  const map: Record<string, string> = {
    js: "javascript",
    jsx: "jsx",
    ts: "typescript",
    tsx: "tsx",
    json: "json",
    css: "css",
    scss: "scss",
    html: "html",
    md: "markdown",
    markdown: "markdown",
    rs: "rust",
    py: "python",
    ps1: "powershell",
    sh: "bash",
    bash: "bash",
    yml: "yaml",
    yaml: "yaml",
    toml: "toml",
    sql: "sql",
    go: "go",
    java: "java",
    c: "c",
    h: "c",
    cpp: "cpp",
    hpp: "cpp",
    cs: "csharp",
  };
  return ext ? (map[ext] ?? ext) : "text";
}

function lineCount(text: string): number {
  if (!text) return 0;
  return text.split("\n").length;
}

/** Last non-empty line of a tool result — used for the compact inline tail. */
function lastNonEmptyLine(text: string): string {
  const lines = text.replace(/\r/g, "").split("\n");
  for (let i = lines.length - 1; i >= 0; i--) {
    const trimmed = lines[i].trim();
    if (trimmed) return trimmed;
  }
  return "";
}

function CodeBlock({
  text,
  language = "text",
  maxHeight = "18rem",
}: {
  text: string;
  language?: string;
  maxHeight?: string;
}) {
  return (
    <div
      className="rounded-md overflow-x-auto overflow-y-auto border border-[var(--ct-border)]"
      style={{ background: "var(--ct-bg)", maxHeight }}
    >
      <SyntaxHighlighter
        language={language}
        style={vscDarkPlus}
        PreTag="div"
        customStyle={{
          margin: 0,
          borderRadius: "0.375rem",
          padding: "0.75rem",
          fontSize: "0.75rem",
          lineHeight: "1.4",
          background: "var(--ct-bg)",
          maxWidth: "100%",
          overflowWrap: "anywhere",
          wordBreak: "break-word",
        }}
        codeTagProps={{
          style: {
            fontFamily: "inherit",
            overflowWrap: "anywhere",
            wordBreak: "break-word",
          },
        }}
        wrapLongLines
      >
        {text.replace(/\n$/, "")}
      </SyntaxHighlighter>
    </div>
  );
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
      parts.push(<pre key={key++} className={`${PRE_WRAP_CLASS} font-[inherit] m-0 select-text`}>{before}</pre>);
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
          background: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 12%, transparent)",
          color: "var(--ct-fg)",
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
    parts.push(<pre key={key++} className={`${PRE_WRAP_CLASS} font-[inherit] m-0 select-text`}>{rest}</pre>);
  }

  // No attachments found — plain text
  if (parts.length === 0) {
    return <pre className={`${PRE_WRAP_CLASS} font-[inherit] m-0 select-text`}>{content}</pre>;
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

/** Renders slash-command blocks (name chip, optional args/stdout). */
function CommandBody({ commands, body }: { commands: SlashCommand[]; body: string }) {
  return (
    <div className="space-y-2">
      {commands.map((c, i) => (
        <div key={i} className="space-y-1">
          <div className="flex items-center gap-2">
            <code
              className="px-2 py-0.5 rounded text-xs font-mono"
              style={{
                background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 18%, transparent)",
                color: "var(--ct-accent, #58a6ff)",
              }}
            >
              {c.name}
            </code>
            {c.args && (
              <span className="text-xs opacity-70 font-mono">{c.args}</span>
            )}
          </div>
          {c.stdout && (
            <pre
              className={`text-xs font-mono px-2 py-1.5 rounded ${PRE_WRAP_CLASS}`}
              style={{
                background: "color-mix(in srgb, var(--ct-bg, #0d1117) 70%, black 5%)",
                color: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 80%, transparent)",
                borderLeft: "2px solid color-mix(in srgb, var(--ct-muted-fg) 40%, transparent)",
              }}
            >
              {c.stdout}
            </pre>
          )}
        </div>
      ))}
      {body.trim() && (
        <div className="text-sm opacity-70">{body}</div>
      )}
    </div>
  );
}

interface MessageBubbleProps {
  message: ChatMessage;
  /** When set, this user message arrived via a codeterm channel (cross-pane,
   *  telegram, mesh, slash-command) and should render as a typed header
   *  instead of the default right-aligned "user" styling. */
  origin?: ChatOrigin;
  /** When set, clicking the origin header focuses the source pane. */
  onOriginClick?: () => void;
}

export const MessageBubble = React.memo(function MessageBubble({ message, origin, onOriginClick }: MessageBubbleProps) {
  // Auto-expand only for Write-style tool input (the file body the agent is
  // writing). A bash/grep result must NOT force-expand — it shows a compact
  // inline tail under the pill instead (full output behind the chevron).
  const shouldAutoExpand = message.type === "tool_call"
    && message.toolInput?.content != null;
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
      const OriginIcon =
        origin.kind === "pane"    ? ArrowRight :
        origin.kind === "mesh"    ? Network :
        origin.kind === "command" ? ChevronsRight :
        Send;
      const label =
        origin.kind === "pane"    ? `from ${origin.title}` :
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
            <span className="font-medium font-mono" style={{ color: "var(--ct-muted-fg)" }}>{message.toolName}</span>
            {summary && (
              <span className="min-w-0 truncate opacity-60 font-mono text-[11px]">{summary}</span>
            )}
            {toolContent && (
              <span className="shrink-0 opacity-40 font-mono text-[11px]">
                {lineCount(toolContent)} lines
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
                  <CodeBlock text={resultText} language="text" maxHeight="12rem" />
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
