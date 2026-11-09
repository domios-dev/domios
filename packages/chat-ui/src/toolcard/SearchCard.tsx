import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { CardFacts } from "../CompactCard";
import { ToolCardFrame } from "./ToolCardFrame";
import { searchModel } from "./searchModel";

export function SearchCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  const result = searchModel(event);
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title={result.title} summary={result.subject} chips={<span data-testid="toolcard-search-counts" className="ct-search-counts shrink-0 font-mono text-[10px]" style={{ color: "var(--ct-muted-fg)" }}><CardFacts full={result.counts} brief={result.fact} /></span>}>
    <SearchDetail event={event} />
  </ToolCardFrame>;
}

export function SearchDetail({ event }: { event: ToolEvent }) {
  return <>
    <div className="break-all font-mono text-xs" style={{ color: "var(--ct-fg)" }}>{event.detail || event.summary}</div>
    {event.status === "pending" ? <div data-testid="toolcard-spinner" style={{ color: "var(--ct-muted-fg)" }}>Searching…</div> : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </>;
}
