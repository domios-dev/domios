import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { parseToolOutputImages } from "../toolImageOutput";
import { ToolCardFrame } from "./ToolCardFrame";

function readableOutput(value: string): string {
  try {
    const parsed: unknown = JSON.parse(value);
    if (parsed && typeof parsed === "object") return "Structured tool output";
    return typeof parsed === "string" ? parsed : String(parsed);
  } catch {
    return value;
  }
}

export function GenericCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  const output = event.result
    ? parseToolOutputImages(event.result).images.length > 0 ? event.result : readableOutput(event.result)
    : undefined;
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle}>
    {event.detail && <div data-testid="toolcard-generic-detail" className="mb-2 whitespace-pre-wrap break-words text-xs" style={{ color: "var(--ct-fg)" }}>{readableOutput(event.detail)}</div>}
    {event.status === "pending" ? <div data-testid="toolcard-spinner" style={{ color: "var(--ct-muted-fg)" }}>Working…</div> : output && <div data-testid="toolcard-result"><ToolOutputContent text={output} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
