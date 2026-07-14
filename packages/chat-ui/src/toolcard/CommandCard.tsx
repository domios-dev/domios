import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";

export function CommandCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title={event.kind === "codeterm" ? "codeterm" : "command"}>
    <div data-testid="toolcard-command" className="font-mono text-xs whitespace-pre-wrap break-words">{event.detail || event.summary || "command"}</div>
    {event.status === "pending" ? <div data-testid="toolcard-spinner" className="mt-2 opacity-60">Running…</div> : event.result && <div data-testid="toolcard-result" className="mt-2"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
