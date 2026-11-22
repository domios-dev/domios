import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { parseToolOutputImages } from "../toolImageOutput";
import { ToolCardFrame } from "./ToolCardFrame";

function readableOutput(value: string): string {
  try {
    const parsed: unknown = JSON.parse(value);
    if (parsed && typeof parsed === "object") return JSON.stringify(parsed, null, 2);
    return typeof parsed === "string" ? parsed : String(parsed);
  } catch {
    return value;
  }
}

export function GenericCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  const output = event.result
    ? parseToolOutputImages(event.result).images.length > 0 ? event.result : readableOutput(event.result)
    : undefined;
  const title = event.title || event.message.toolName;
  const summary = event.description || event.summary || (title ? "" : undefined);
  return <ToolCardFrame event={event} title={title} summary={summary} expanded={expanded} onToggle={onToggle}>
    {event.detail && <div data-testid="toolcard-generic-detail" className="whitespace-pre-wrap break-words text-xs" style={{ color: "var(--ct-fg)" }}>{readableOutput(event.detail)}</div>}
    {event.status === "pending" && (
      <div data-testid="toolcard-spinner" className="flex items-center gap-1.5 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
        <span aria-hidden="true" className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--ct-accent)" }} />
        <span>Working…</span>
      </div>
    )}
    {output && <div data-testid="toolcard-result" className={event.status === "pending" ? "mt-1.5" : undefined}><ToolOutputContent text={output} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
