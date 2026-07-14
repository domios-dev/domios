import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";

export function GenericToolCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  const title = event.kind === "read" ? "read" : event.kind === "search" ? "search" : event.kind === "task" ? "task" : event.kind === "fetch" ? "fetch" : event.kind === "codeterm" ? "codeterm" : "tool";
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title={title}>
    {event.detail && <div className="text-xs whitespace-pre-wrap break-words">{event.detail}</div>}
    {event.status === "pending" ? <div data-testid="toolcard-spinner" className="opacity-60">Running…</div> : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
