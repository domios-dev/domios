import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";

function readTargets(event: ToolEvent): string {
  const command = event.detail || event.summary;
  const paths = [...command.matchAll(/(?:^|\s)(?:["']?)([./~\w@-]+\/[\w@.-]+|[\w@.-]+\.(?:rs|ts|tsx|js|jsx|json|md|toml|yml|yaml|css|html|sh|ps1))(?:["']?)(?=\s|$)/g)]
    .map((match) => {
      const parts = match[1].split("/");
      return parts[parts.length - 1];
    })
    .filter((path, index, all) => all.indexOf(path) === index);
  if (paths.length === 0) return "file contents";
  return paths.length > 3 ? `${paths.slice(0, 3).join(", ")} +${paths.length - 3}` : paths.join(", ");
}

export function ReadCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title="Read" summary={readTargets(event)}>
    {event.detail && <div data-testid="toolcard-read-path" className="mb-2 break-all font-mono text-xs" style={{ color: "var(--ct-fg)" }}>{event.detail}</div>}
    {event.status === "pending" ? <div data-testid="toolcard-spinner" style={{ color: "var(--ct-muted-fg)" }}>Reading…</div> : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
