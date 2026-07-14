import type { ToolEvent } from "@codeterm/chat-core";
import { ChevronDown } from "lucide-react";
import type * as React from "react";

export function ToolCardFrame({ event, expanded, onToggle, children, title, metric }: { event: ToolEvent; expanded: boolean; onToggle: () => void; children: React.ReactNode; title: string; metric?: React.ReactNode }) {
  const failed = event.status === "error";
  return <div data-testid="chat-tool-call" className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto rounded-md border" style={{ borderStyle: failed ? "dashed" : "solid", borderColor: "var(--ct-border)", background: "var(--ct-surface)" }}>
    <button type="button" onClick={onToggle} className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs">
      <span data-testid="toolcard-status" className={event.status === "pending" ? "animate-spin" : ""}>{failed ? "✕" : event.status === "pending" ? "◜" : "✓"}</span>
      <span className="font-medium">{title}</span><span className="min-w-0 flex-1 truncate opacity-70">{event.summary}</span>{metric}
      <ChevronDown size={13} className={expanded ? "rotate-180" : ""} />
    </button>
    {expanded && <div className="border-t px-2.5 py-2" style={{ borderColor: "var(--ct-border)" }}>{children}</div>}
  </div>;
}
