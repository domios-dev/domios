import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";
import { toolKindVisual } from "./toolKindVisual";

export function TaskCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title={toolKindVisual.task.label}>
    {event.detail && <div data-testid="toolcard-task-detail" className="mb-2 whitespace-pre-wrap break-words text-xs" style={{ color: "var(--ct-fg)" }}>{event.detail}</div>}
    {event.status === "pending" ? <div data-testid="toolcard-spinner" style={{ color: "var(--ct-muted-fg)" }}>Working…</div> : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
