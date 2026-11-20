import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { FilePathLabel } from "./FileDiffCards";
import { ToolCardFrame } from "./ToolCardFrame";

function readTargets(event: ToolEvent) {
  const directPath = [event.detail, event.summary].map((value) => value?.trim() ?? "").find((value) =>
    value && !/[\r\n]/.test(value) && (/^[A-Za-z]:[\\/]/.test(value) || (!/\s/.test(value) && /[\\/]/.test(value))),
  );
  if (directPath) return <FilePathLabel path={directPath} />;

  const command = event.detail || event.summary;
  const paths = [...command.matchAll(/(?:^|\s)(?:["']?)([./~\w@-]+\/[\w@.-]+|[\w@.-]+\.(?:rs|ts|tsx|js|jsx|json|md|toml|yml|yaml|css|html|sh|ps1))(?:["']?)(?=\s|$)/g)]
    .map((match) => match[1])
    .filter((path, index, all) => all.indexOf(path) === index);
  if (paths.length === 0) return "file contents";
  const visible = paths.slice(0, 3);
  return <>{visible.map((path, index) => <span key={path}>{index > 0 && ", "}<FilePathLabel path={path} /></span>)}{paths.length > 3 ? ` +${paths.length - 3}` : ""}</>;
}

export function ReadCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title="Read" summary={readTargets(event)}>
    <ReadDetail event={event} />
  </ToolCardFrame>;
}

export function ReadDetail({ event }: { event: ToolEvent }) {
  return <>
    {event.detail && <div data-testid="toolcard-read-path" className="break-all font-mono text-xs" style={{ color: "var(--ct-fg)" }}>{event.detail}</div>}
    {event.status === "pending" ? <div data-testid="toolcard-spinner" style={{ color: "var(--ct-muted-fg)" }}>Reading…</div> : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </>;
}
