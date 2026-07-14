import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";
import { toolKindVisual } from "./toolKindVisual";

export function CommandCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title={toolKindVisual[event.kind].label}>
    <div data-testid="toolcard-command" className="font-mono text-xs whitespace-pre-wrap break-words" style={{ color: "var(--ct-fg)" }}>{event.detail || event.summary || toolKindVisual[event.kind].label}</div>
    {event.status === "pending" ? <div data-testid="toolcard-spinner" className="mt-2" style={{ color: "var(--ct-muted-fg)" }}>Running…</div> : event.result && <div data-testid="toolcard-result" className="mt-2"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
