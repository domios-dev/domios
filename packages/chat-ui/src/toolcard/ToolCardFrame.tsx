import type { ToolEvent } from "@codeterm/chat-core";
import { ChevronDown, Check, CircleX, LoaderCircle, ChevronRight } from "lucide-react";
import { useState, type ReactNode } from "react";
import { toolKindVisual, useShowCommands } from "./toolKindVisual";

interface ToolCardFrameProps {
  event: ToolEvent;
  expanded: boolean;
  onToggle: () => void;
  children: ReactNode;
  title?: string;
  metric?: ReactNode;
}

export function ToolCardFrame({ event, expanded, onToggle, children, title, metric }: ToolCardFrameProps) {
  const [commandOpen, setCommandOpen] = useState(false);
  const globalShowCommands = useShowCommands();
  const visual = toolKindVisual[event.kind];
  const Glyph = visual.glyph;
  const isCommand = event.kind === "command" || event.kind === "codeterm";
  const commandVisible = !isCommand || globalShowCommands || commandOpen || expanded;
  const statusColor = event.status === "error" ? "var(--ct-error)" : event.status === "pending" ? "var(--ct-warning)" : "var(--ct-success)";

  return (
    <div
      data-testid="chat-tool-call"
      data-tool-status={event.status}
      className="mx-auto mb-1.5 w-full max-w-4xl min-w-0 rounded-md border"
      style={{ borderStyle: event.status === "error" ? "dashed" : "solid", borderColor: "var(--ct-border)", background: "var(--ct-surface)" }}
    >
      <div className="flex min-w-0 items-center gap-2 px-2.5 py-1.5 text-left text-xs">
        <Glyph aria-hidden="true" size={14} strokeWidth={2} style={{ color: visual.color, flexShrink: 0 }} />
        <span className="font-medium" style={{ color: visual.color }}>{title ?? visual.label}</span>
        <span className="min-w-0 flex-1 truncate" style={{ color: "var(--ct-fg-muted)" }}>{event.summary || "Tool activity"}</span>
        {metric}
        <span data-testid="toolcard-status" aria-label={event.status} style={{ color: statusColor }}>
          {event.status === "pending" ? <LoaderCircle className="animate-spin" size={14} /> : event.status === "error" ? <CircleX size={14} /> : <Check size={14} />}
        </span>
        {isCommand && (
          <button
            type="button"
            aria-label={commandOpen ? "Hide command" : "Show command"}
            data-testid="toolcard-command-open"
            aria-expanded={commandVisible}
            onClick={() => setCommandOpen((open) => !open)}
            className="rounded-sm p-0.5"
            style={{ color: "var(--ct-fg-muted)" }}
          >
            <ChevronRight size={14} className={commandVisible ? "rotate-90" : ""} />
          </button>
        )}
        <button type="button" aria-label={expanded ? "Collapse tool details" : "Expand tool details"} onClick={onToggle} className="rounded-sm p-0.5" style={{ color: "var(--ct-fg-muted)" }}>
          <ChevronDown size={13} className={expanded ? "rotate-180" : ""} />
        </button>
      </div>
      {(expanded || (isCommand && commandVisible)) && <div className="border-t px-2.5 py-2" style={{ borderColor: "var(--ct-border)" }}>{children}</div>}
    </div>
  );
}
