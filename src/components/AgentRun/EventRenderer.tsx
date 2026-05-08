import { useState } from "react";
import type {
  ClaudeContentBlock,
  ClaudeStreamEvent,
} from "./streamParser";

interface Props {
  event: ClaudeStreamEvent;
}

export function EventRow({ event }: Props) {
  switch (event.type) {
    case "system":
      return <SystemRow event={event} />;
    case "assistant":
      return <AssistantRow event={event} />;
    case "user":
      return <UserRow event={event} />;
    case "result":
      return <ResultRow event={event} />;
    case "rate_limit":
    case "unknown":
      // Drop SDK-internal deltas (stream_event, content_block_delta, etc) and
      // rate limit chatter — the high-level assistant/user/result events
      // already carry the full content, no need to clutter the timeline.
      return null;
  }
}

function SystemRow({
  event,
}: {
  event: Extract<ClaudeStreamEvent, { type: "system" }>;
}) {
  if (event.subtype !== "init") return null;
  return (
    <div className="flex items-center gap-2 text-[10px] text-[var(--ct-muted-fg)] py-0.5">
      <span>◉</span>
      <span>session started</span>
      {event.model && <span className="opacity-60">· {event.model}</span>}
    </div>
  );
}

function AssistantRow({
  event,
}: {
  event: Extract<ClaudeStreamEvent, { type: "assistant" }>;
}) {
  return (
    <div className="flex flex-col gap-1 py-1">
      {event.content.map((block, i) => (
        <ContentBlockView key={i} block={block} role="assistant" />
      ))}
    </div>
  );
}

function UserRow({
  event,
}: {
  event: Extract<ClaudeStreamEvent, { type: "user" }>;
}) {
  return (
    <div className="flex flex-col gap-1 py-1">
      {event.content.map((block, i) => (
        <ContentBlockView key={i} block={block} role="user" />
      ))}
    </div>
  );
}

function ResultRow({
  event,
}: {
  event: Extract<ClaudeStreamEvent, { type: "result" }>;
}) {
  const color = event.is_error ? "var(--ct-red)" : "var(--ct-green)";
  return (
    <div
      className="flex items-center gap-2 text-[10px] py-1 mt-1 border-t border-[var(--ct-border)]"
      style={{ color }}
    >
      <span>{event.is_error ? "✗" : "✓"}</span>
      <span>{event.is_error ? "failed" : event.subtype ?? "done"}</span>
      {event.duration_ms !== undefined && (
        <span className="opacity-70">
          · {(event.duration_ms / 1000).toFixed(1)}s
        </span>
      )}
      {event.total_cost_usd !== undefined && (
        <span className="opacity-70">
          · ${event.total_cost_usd.toFixed(4)}
        </span>
      )}
      {event.result && (
        <span className="opacity-90 truncate flex-1">
          → {event.result.slice(0, 200)}
        </span>
      )}
    </div>
  );
}

function ContentBlockView({
  block,
  role,
}: {
  block: ClaudeContentBlock;
  role: "assistant" | "user";
}) {
  if (block.type === "text") {
    return (
      <div className="text-[11px] text-[var(--ct-fg)] whitespace-pre-wrap break-words leading-relaxed">
        {block.text}
      </div>
    );
  }

  if (block.type === "thinking") {
    return <Thinking text={block.thinking} />;
  }

  if (block.type === "tool_use") {
    return <ToolUse name={block.name} input={block.input} />;
  }

  if (block.type === "tool_result") {
    return <ToolResult content={block.content} isError={block.is_error} />;
  }

  return (
    <div className="text-[10px] text-[var(--ct-muted-fg)] opacity-60">
      · unknown {role} content
    </div>
  );
}

function Thinking({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  const preview = text.slice(0, 240).replace(/\n+/g, " ");
  const truncated = text.length > 240;
  return (
    <div className="text-[10px] text-[var(--ct-muted-fg)] italic">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="bg-transparent border-0 text-left text-[var(--ct-muted-fg)] cursor-pointer hover:text-[var(--ct-fg)] p-0 leading-snug"
      >
        <span>{open ? "▾" : "▸"} thinking</span>
        {!open && (
          <span className="opacity-80 ml-1">
            {preview}
            {truncated && "…"}
          </span>
        )}
      </button>
      {open && (
        <pre className="mt-1 p-2 bg-[var(--ct-bg-alt,rgba(255,255,255,0.03))] rounded text-[10px] whitespace-pre-wrap break-words">
          {text}
        </pre>
      )}
    </div>
  );
}

function ToolUse({ name, input }: { name: string; input: unknown }) {
  const [open, setOpen] = useState(false);
  const summary = summarizeToolInput(name, input);
  return (
    <div className="text-[11px]">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="bg-transparent border-0 text-left cursor-pointer flex items-start gap-1.5 p-0 text-[var(--ct-fg)] hover:text-[var(--ct-accent)] w-full"
      >
        <span className="text-[var(--ct-accent)] shrink-0">●</span>
        <span className="font-medium shrink-0">{name}</span>
        {summary && (
          <span className="text-[var(--ct-muted-fg)] break-all flex-1 min-w-0">
            ({summary})
          </span>
        )}
        <span className="text-[9px] text-[var(--ct-muted-fg)] shrink-0">
          {open ? "▾" : "▸"}
        </span>
      </button>
      {open && (
        <pre className="mt-1 ml-3.5 p-2 bg-[var(--ct-bg-alt,rgba(255,255,255,0.03))] rounded text-[10px] whitespace-pre-wrap break-words text-[var(--ct-muted-fg)]">
          {JSON.stringify(input, null, 2)}
        </pre>
      )}
    </div>
  );
}

function ToolResult({
  content,
  isError,
}: {
  content: unknown;
  isError?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const text = stringifyToolResult(content);
  const preview = text.slice(0, 280).replace(/\n+/g, " ⏎ ");
  const truncated = text.length > 280;
  const color = isError ? "var(--ct-red)" : "var(--ct-muted-fg)";
  return (
    <div className="text-[10px] ml-3.5">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="bg-transparent border-0 text-left cursor-pointer flex items-start gap-1.5 p-0 w-full"
        style={{ color }}
      >
        <span className="shrink-0">⎿</span>
        <span className="opacity-95 break-all flex-1 min-w-0">
          {isError ? "Error: " : ""}
          {open ? "" : preview}
          {!open && truncated && "…"}
        </span>
      </button>
      {open && (
        <pre className="mt-1 p-2 bg-[var(--ct-bg-alt,rgba(255,255,255,0.03))] rounded whitespace-pre-wrap break-words" style={{ color }}>
          {text}
        </pre>
      )}
    </div>
  );
}

function summarizeToolInput(name: string, input: unknown): string {
  if (!input || typeof input !== "object") return "";
  const i = input as Record<string, unknown>;
  if (name === "Bash" && typeof i.command === "string") {
    return i.command.slice(0, 80);
  }
  if ((name === "Read" || name === "Edit" || name === "Write") && typeof i.file_path === "string") {
    return i.file_path;
  }
  if (name === "Glob" && typeof i.pattern === "string") {
    return i.pattern;
  }
  if (name === "Grep" && typeof i.pattern === "string") {
    return i.pattern;
  }
  // Fallback: first string value
  for (const v of Object.values(i)) {
    if (typeof v === "string" && v.length > 0) return v.slice(0, 80);
  }
  return "";
}

function stringifyToolResult(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    // Claude tool results often come as `[{type:"text", text:"..."}]`
    return content
      .map((c) => {
        if (c && typeof c === "object" && "text" in c && typeof (c as { text: unknown }).text === "string") {
          return (c as { text: string }).text;
        }
        return JSON.stringify(c);
      })
      .join("\n");
  }
  if (content && typeof content === "object") return JSON.stringify(content, null, 2);
  return String(content ?? "");
}
