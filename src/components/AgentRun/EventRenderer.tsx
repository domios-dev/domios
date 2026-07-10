import { useState } from "react";
import type {
  ClaudeContentBlock,
  ClaudeStreamEvent,
} from "./streamParser";
import { stringifyToolPayload, ToolOutputCard } from "./ToolOutputCard";

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
      // Drop SDK-internal deltas (stream_event, content_block_delta, etc) and rate limit chatter — the high-level assistant/user/result events
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
        <span className="opacity-90 min-w-0 overflow-hidden text-ellipsis whitespace-nowrap flex-1">
          → {event.result}
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
  return (
    <ToolOutputCard
      kind="call"
      toolName={name}
      payload={JSON.stringify(input, null, 2) ?? String(input ?? "")}
    />
  );
}

function ToolResult({
  content,
  isError,
}: {
  content: unknown;
  isError?: boolean;
}) {
  return (
    <ToolOutputCard kind="result" payload={stringifyToolPayload(content)} isError={isError} />
  );
}
