import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";
import { toolKindVisual } from "./toolKindVisual";

function resultTail(result: string | undefined): string | undefined {
  const lines = result?.split("\n").map((line) => line.trim()).filter(Boolean) ?? [];
  return lines[lines.length - 1];
}

export function CommandCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  const tail = resultTail(event.result);
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title={toolKindVisual[event.kind].label} disclosureLabel="full command" tail={tail && <span data-testid="toolcard-result-tail" className="max-w-[34%] truncate font-mono text-[10px]" style={{ color: "var(--ct-muted-fg)" }}>→ {tail}</span>} command={
    <div data-testid="toolcard-command" role="region" aria-label="Full command text" tabIndex={0} className="overflow-auto border-t px-3 py-2 font-mono text-xs whitespace-pre-wrap break-words focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--ct-accent)]" style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)", background: "var(--ct-bg)", maxHeight: "12rem" }}>{event.detail || event.summary || toolKindVisual[event.kind].label}</div>
  }>
    {event.status === "pending" ? <div data-testid="toolcard-spinner" className="mt-2" style={{ color: "var(--ct-muted-fg)" }}>Running…</div> : event.result && <div data-testid="toolcard-result" className="mt-2"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
