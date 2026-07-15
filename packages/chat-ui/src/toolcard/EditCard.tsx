import type { ToolEdit, ToolEvent } from "@codeterm/chat-core";
import { countAdded, countRemoved } from "@codeterm/chat-core";
import { DiffView } from "../MarkdownBody";
import { languageForEdits } from "../codeLanguage";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";

function groupedEdits(edits: ToolEdit[]) {
  const groups = new Map<string, ToolEdit[]>();
  for (const edit of edits) {
    groups.set(edit.path, [...(groups.get(edit.path) ?? []), edit]);
  }
  return groups;
}

export function EditCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  const edits = event.edits ?? [];
  const added = edits.reduce((n, edit) => n + countAdded(edit), 0);
  const removed = edits.reduce((n, edit) => n + countRemoved(edit), 0);
  const groups = groupedEdits(edits);
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} metric={<span data-testid="toolcard-edit-summary" className="shrink-0 font-mono text-[10px]"><span style={{ color: "var(--ct-green)" }}>+{added}</span> <span style={{ color: "var(--ct-red)" }}>-{removed}</span></span>}>
    {expanded && Array.from(groups, ([path, fileEdits]) => <section key={path} data-testid="toolcard-edit-file" className="mt-2">
      <div className="mb-1 font-mono text-xs" style={{ color: "var(--ct-fg)" }}>{path} <span style={{ color: "var(--ct-muted-fg)" }}>+{fileEdits.reduce((n, edit) => n + countAdded(edit), 0)} -{fileEdits.reduce((n, edit) => n + countRemoved(edit), 0)}</span></div>
      <DiffView edits={fileEdits} language={languageForEdits(path, fileEdits)} />
    </section>)}
    {event.status === "error" && event.result && <div data-testid="toolcard-result" className="mt-2"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
