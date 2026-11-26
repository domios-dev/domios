import React, { useId, useState } from "react";
import { History } from "lucide-react";
import { taskStridesLabel, type TaskStridesBlockData } from "@codeterm/chat-core";
import { ChevronIcon, PRE_WRAP_CLASS } from "./CodeBlock";

interface Props {
  block: TaskStridesBlockData;
  renderBody?: (body: string) => React.ReactNode;
}

function renderInlineCodeLine(line: string): React.ReactNode {
  const parts = line.split(/(`[^`]+`)/g);
  if (parts.length === 1) return line;
  return parts.map((part, index) => {
    if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
      return (
        <code
          key={index}
          className="rounded px-1 py-0.5 font-mono text-[0.9em]"
          style={{
            background: "color-mix(in srgb, var(--ct-fg) 7%, transparent)",
            color: "var(--ct-fg)",
          }}
        >
          {part.slice(1, -1)}
        </code>
      );
    }
    return <React.Fragment key={index}>{part}</React.Fragment>;
  });
}

function DefaultStridesBody({ body }: { body: string }) {
  const lines = body.replace(/\r\n?/g, "\n").split("\n").filter((line) => line.trim().length > 0);
  return (
    <div className={`space-y-1 text-xs leading-relaxed ${PRE_WRAP_CLASS}`}>
      {lines.map((line, index) => (
        <div key={index} className="min-w-0">
          {renderInlineCodeLine(line)}
        </div>
      ))}
    </div>
  );
}

export function TaskStridesBlock({ block, renderBody }: Props) {
  const [expanded, setExpanded] = useState(false);
  const bodyId = useId();
  const label = taskStridesLabel(block.count);

  return (
    <div
      data-testid="chat-task-strides"
      data-stride-count={String(block.count)}
      data-task-id={block.taskId}
      className="my-1.5 w-full min-w-0 rounded-md border"
      style={{
        background: "color-mix(in srgb, var(--ct-surface, #161b22) 88%, transparent)",
        borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
      }}
    >
      <button
        type="button"
        data-testid="chat-task-strides-toggle"
        aria-expanded={expanded}
        aria-controls={bodyId}
        onClick={() => setExpanded((value) => !value)}
        className="flex w-full min-w-0 items-center gap-2 px-2.5 py-1.5 text-left text-xs"
        style={{
          color: "var(--ct-muted-fg)",
          background: "transparent",
          border: "none",
        }}
      >
        <History
          size={12}
          strokeWidth={2}
          aria-hidden="true"
          className="shrink-0"
          style={{ color: "var(--ct-accent, #58a6ff)" }}
        />
        <span className="min-w-0 flex-1 truncate font-medium" style={{ color: "var(--ct-fg)" }}>
          {label}
        </span>
        <ChevronIcon expanded={expanded} className="shrink-0 opacity-60" />
      </button>
      {expanded && (
        <div
          id={bodyId}
          data-testid="chat-task-strides-body"
          className="border-t px-3 py-2 text-xs"
          style={{
            borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 16%, transparent)",
            color: "var(--ct-fg)",
          }}
        >
          {renderBody ? renderBody(block.body) : <DefaultStridesBody body={block.body} />}
        </div>
      )}
    </div>
  );
}
