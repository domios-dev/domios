import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";

function counts(event: ToolEvent): { matches: string; files: string } {
  const text = [event.summary, event.detail, event.result].filter(Boolean).join("\n");
  const matches = text.match(/(\d+)\s+matches?/i)?.[1] ?? text.match(/(\d+)\s+results?/i)?.[1];
  const files = text.match(/(\d+)\s+files?/i)?.[1];
  return { matches: matches ?? "0", files: files ?? "0" };
}

export function SearchCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  const result = counts(event);
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title="⌕" chips={<span data-testid="toolcard-search-counts" className="shrink-0 rounded px-1.5 py-0.5 font-mono text-[10px]" style={{ color: "var(--ct-muted-fg)", background: "var(--ct-border)" }}>{result.matches} matches · {result.files} files</span>}>
    {event.status === "pending" ? <div data-testid="toolcard-spinner" style={{ color: "var(--ct-muted-fg)" }}>Searching…</div> : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
