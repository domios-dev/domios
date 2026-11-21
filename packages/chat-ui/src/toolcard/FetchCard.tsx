import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";

export function FetchCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  const title = event.title || undefined;
  const summary = event.description || event.summary || (title ? "" : undefined);
  return <ToolCardFrame event={event} title={title} summary={summary} expanded={expanded} onToggle={onToggle}>
    {event.detail && <div data-testid="toolcard-fetch-target" className="break-all font-mono text-xs" style={{ color: "var(--ct-fg)" }}>{event.detail}</div>}
    {event.status === "pending" && (
      <div data-testid="toolcard-spinner" className="flex items-center gap-1.5 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
        <span aria-hidden="true" className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--ct-accent)" }} />
        <span>Fetching…</span>
      </div>
    )}
    {event.result && <div data-testid="toolcard-result" className={event.status === "pending" ? "mt-1.5" : undefined}><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
