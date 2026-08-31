import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";

function modelFrom(event: ToolEvent): string | undefined {
  const text = [event.summary, event.detail].filter(Boolean).join("\n");
  return text.match(/(?:--model|model(?:\s+id)?)\s*[:=]?\s*([^\s,;]+)/i)?.[1];
}

export function TaskCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  const model = modelFrom(event);
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title="spawn" chips={model && <span data-testid="toolcard-task-model" className="shrink-0 rounded px-1.5 py-0.5 font-mono text-[10px]" style={{ color: "var(--ct-muted-fg)", background: "var(--ct-border)" }}>{model}</span>}>
    {event.detail && <div data-testid="toolcard-task-detail" className="whitespace-pre-wrap break-words text-xs" style={{ color: "var(--ct-fg)" }}>{event.detail}</div>}
    {event.status === "pending" ? <div data-testid="toolcard-spinner" style={{ color: "var(--ct-muted-fg)" }}>Working…</div> : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
