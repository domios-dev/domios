import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";
import { toolKindVisual } from "./toolKindVisual";

export function FetchCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title={toolKindVisual.fetch.label}>
    {event.detail && <div data-testid="toolcard-fetch-target" className="mb-2 break-all font-mono text-xs" style={{ color: "var(--ct-fg)" }}>{event.detail}</div>}
    {event.status === "pending" ? <div data-testid="toolcard-spinner" style={{ color: "var(--ct-muted-fg)" }}>Fetching…</div> : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
