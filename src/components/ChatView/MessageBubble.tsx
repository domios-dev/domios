import { useState, useMemo } from "react";
import type { ChatMessage } from "./parser";

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

/** Lightweight markdown-lite renderer for assistant text. */
function renderMarkdownLite(text: string): React.ReactNode[] {
  const lines = text.split("\n");
  const nodes: React.ReactNode[] = [];
  let inCodeBlock = false;
  let codeBlockLines: string[] = [];
  let codeBlockKey = 0;

  function flushCodeBlock() {
    if (codeBlockLines.length > 0) {
      nodes.push(
        <pre
          key={`code-${codeBlockKey++}`}
          className="bg-[var(--ct-bg)] rounded-md px-3 py-2 my-1 text-xs overflow-x-auto font-mono"
        >
          {codeBlockLines.join("\n")}
        </pre>,
      );
      codeBlockLines = [];
    }
  }

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Fenced code block start/end
    if (line.trimStart().startsWith("```")) {
      if (inCodeBlock) {
        flushCodeBlock();
        inCodeBlock = false;
      } else {
        inCodeBlock = true;
      }
      continue;
    }

    if (inCodeBlock) {
      codeBlockLines.push(line);
      continue;
    }

    // Bullet list item
    if (/^\s*[-*]\s/.test(line)) {
      const content = line.replace(/^\s*[-*]\s/, "");
      nodes.push(
        <div key={i} className="flex gap-1.5 ml-2">
          <span className="text-[var(--ct-muted)] select-none">&bull;</span>
          <span>{renderInlineMarkdown(content)}</span>
        </div>,
      );
      continue;
    }

    // Numbered list item
    if (/^\s*\d+\.\s/.test(line)) {
      const match = line.match(/^\s*(\d+)\.\s(.*)/);
      if (match) {
        nodes.push(
          <div key={i} className="flex gap-1.5 ml-2">
            <span className="text-[var(--ct-muted)] select-none">{match[1]}.</span>
            <span>{renderInlineMarkdown(match[2])}</span>
          </div>,
        );
        continue;
      }
    }

    // Empty line
    if (!line.trim()) {
      nodes.push(<div key={i} className="h-2" />);
      continue;
    }

    // Regular text line
    nodes.push(
      <div key={i}>{renderInlineMarkdown(line)}</div>,
    );
  }

  // Flush any unclosed code block
  if (inCodeBlock) flushCodeBlock();

  return nodes;
}

/** Render inline markdown: bold, inline code, links. */
function renderInlineMarkdown(text: string): React.ReactNode {
  // Split on inline patterns: **bold**, `code`, [text](url)
  const parts: React.ReactNode[] = [];
  // Combined regex for bold, inline code, and links
  const re = /(\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\(([^)]+)\))/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = re.exec(text)) !== null) {
    // Text before match
    if (match.index > lastIndex) {
      parts.push(text.slice(lastIndex, match.index));
    }

    if (match[2] !== undefined) {
      // Bold
      parts.push(
        <strong key={match.index} className="font-semibold">
          {match[2]}
        </strong>,
      );
    } else if (match[3] !== undefined) {
      // Inline code
      parts.push(
        <code
          key={match.index}
          className="bg-[var(--ct-bg)] px-1 py-0.5 rounded text-[0.85em] font-mono"
        >
          {match[3]}
        </code>,
      );
    } else if (match[4] !== undefined && match[5] !== undefined) {
      // Link
      parts.push(
        <a
          key={match.index}
          href={match[5]}
          className="text-[var(--ct-blue)] underline"
          target="_blank"
          rel="noopener noreferrer"
        >
          {match[4]}
        </a>,
      );
    }

    lastIndex = match.index + match[0].length;
  }

  // Remaining text
  if (lastIndex < text.length) {
    parts.push(text.slice(lastIndex));
  }

  return parts.length === 1 ? parts[0] : <>{parts}</>;
}

/** Truncate tool args for summary display. */
function summarizeToolArgs(args: string, maxLen = 80): string {
  if (!args) return "";
  const firstLine = args.split("\n")[0].trim();
  if (firstLine.length <= maxLen) return firstLine;
  return firstLine.slice(0, maxLen) + "...";
}

export function MessageBubble({ message }: { message: ChatMessage }) {
  const [expanded, setExpanded] = useState(false);

  const renderedContent = useMemo(() => {
    if (message.type === "assistant") {
      return renderMarkdownLite(message.content);
    }
    return null;
  }, [message.type, message.content]);

  // User message
  if (message.type === "user") {
    return (
      <div className="flex justify-end mb-3 px-4">
        <div
          className="max-w-[85%] rounded-2xl rounded-br-md px-4 py-2.5 text-sm"
          style={{
            background: "rgba(var(--ct-blue-rgb, 88, 166, 255), 0.15)",
            color: "var(--ct-fg)",
          }}
        >
          <pre className="whitespace-pre-wrap font-[inherit] m-0">{message.content}</pre>
        </div>
      </div>
    );
  }

  // Assistant message
  if (message.type === "assistant") {
    return (
      <div className="flex justify-start mb-3 px-4">
        <div
          className="max-w-[85%] rounded-2xl rounded-bl-md px-4 py-2.5 text-sm leading-relaxed"
          style={{
            background: "var(--ct-surface, #161b22)",
            color: "var(--ct-fg)",
          }}
        >
          {renderedContent}
        </div>
      </div>
    );
  }

  // Tool call
  if (message.type === "tool_call") {
    const icon = TOOL_ICONS[message.toolName || ""] || "\u2699\uFE0F";
    const summary = summarizeToolArgs(message.toolArgs || "");

    return (
      <div className="mb-2 px-4">
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-2 w-full text-left rounded-lg px-3 py-2 text-xs transition-colors duration-150 hover:bg-[var(--ct-bg-hover)]"
          style={{
            background: "rgba(var(--ct-fg-rgb, 201, 209, 217), 0.04)",
            color: "var(--ct-muted, #8b949e)",
          }}
        >
          <span className="text-sm flex-shrink-0">{icon}</span>
          <span className="font-medium text-[var(--ct-fg)]">{message.toolName}</span>
          {summary && (
            <span className="truncate opacity-60 font-mono">{summary}</span>
          )}
          <svg
            className={`ml-auto w-3 h-3 flex-shrink-0 transition-transform duration-150 ${expanded ? "rotate-180" : ""}`}
            viewBox="0 0 12 12"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
          >
            <path d="M3 4.5L6 7.5L9 4.5" />
          </svg>
        </button>
        {expanded && (
          <div
            className="mt-1 ml-7 rounded-md px-3 py-2 text-xs font-mono overflow-x-auto max-h-60 overflow-y-auto"
            style={{ background: "var(--ct-bg, #0d1117)", color: "var(--ct-fg)" }}
          >
            <pre className="whitespace-pre-wrap m-0">{message.toolArgs || message.content}</pre>
          </div>
        )}
      </div>
    );
  }

  // Tool result
  if (message.type === "tool_result") {
    const contentLines = message.content.split("\n");
    const preview = contentLines.slice(0, 2).join("\n");
    const hasMore = contentLines.length > 2;

    return (
      <div className="mb-2 px-4 ml-4">
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex items-start gap-2 w-full text-left rounded-md px-3 py-1.5 text-xs transition-colors duration-150 hover:bg-[var(--ct-bg-hover)]"
          style={{
            color: "var(--ct-muted, #8b949e)",
          }}
        >
          <span className="opacity-50 flex-shrink-0 mt-0.5">{"\u23BF"}</span>
          <span className="opacity-70 font-mono overflow-hidden" style={{ display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical" as const }}>
            {preview}
            {hasMore && !expanded ? "..." : ""}
          </span>
          {hasMore && (
            <svg
              className={`ml-auto w-3 h-3 flex-shrink-0 mt-0.5 transition-transform duration-150 ${expanded ? "rotate-180" : ""}`}
              viewBox="0 0 12 12"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
            >
              <path d="M3 4.5L6 7.5L9 4.5" />
            </svg>
          )}
        </button>
        {expanded && (
          <div
            className="mt-1 ml-5 rounded-md px-3 py-2 text-xs font-mono overflow-x-auto max-h-60 overflow-y-auto"
            style={{ background: "var(--ct-bg, #0d1117)", color: "var(--ct-fg)" }}
          >
            <pre className="whitespace-pre-wrap m-0">{message.content}</pre>
          </div>
        )}
      </div>
    );
  }

  // Thinking block
  if (message.type === "thinking") {
    return (
      <div className="mb-2 px-4">
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-2 w-full text-left rounded-lg px-3 py-1.5 text-xs transition-colors duration-150 hover:bg-[var(--ct-bg-hover)]"
          style={{
            background: "rgba(var(--ct-fg-rgb, 201, 209, 217), 0.03)",
            color: "var(--ct-muted, #8b949e)",
          }}
        >
          <span className="text-sm flex-shrink-0 opacity-50">{"..."}</span>
          <span className="font-medium opacity-70">Thinking</span>
          <svg
            className={`ml-auto w-3 h-3 flex-shrink-0 transition-transform duration-150 ${expanded ? "rotate-180" : ""}`}
            viewBox="0 0 12 12"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
          >
            <path d="M3 4.5L6 7.5L9 4.5" />
          </svg>
        </button>
        {expanded && (
          <div
            className="mt-1 ml-7 rounded-md px-3 py-2 text-xs overflow-x-auto max-h-60 overflow-y-auto italic"
            style={{
              background: "var(--ct-bg, #0d1117)",
              color: "var(--ct-muted, #8b949e)",
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
      <div className="flex justify-center mb-3 px-4">
        <span
          className="text-[10px] px-3 py-1 rounded-full"
          style={{
            color: "var(--ct-muted, #8b949e)",
            background: "rgba(var(--ct-fg-rgb, 201, 209, 217), 0.04)",
          }}
        >
          {message.content}
        </span>
      </div>
    );
  }

  return null;
}
