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
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title={toolKindVisual[event.kind].label} disclosureLabel="full command" tail={tail && <span data-testid="toolcard-result-tail" className="max-w-[34%] truncate font-mono text-[10px]" style={{ color: "var(--ct-muted-fg)" }}>→ {tail}</span>}>
    {event.status === "pending" ? <div data-testid="toolcard-spinner" style={{ color: "var(--ct-muted-fg)" }}>Running…</div> : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
