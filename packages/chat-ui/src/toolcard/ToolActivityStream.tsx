import { toolStreamLabel, type ToolEvent } from "@codeterm/chat-core";
import { Check, ChevronRight, CircleX, LoaderCircle, Minus } from "lucide-react";
import { useId, useState, type ReactNode } from "react";
import { ThoughtLine } from "./ThoughtLine";
import { ToolOutcomeFacts } from "./ToolOutcomeFacts";

const MINI_VIEW_MAX_PX = 200;
const MINI_VIEW_ROWS = 6;

export type RenderStreamAction = (member: ToolEvent, expanded: boolean, onToggle: (next: boolean) => void) => ReactNode;

function statusColor(status: ToolEvent["status"]): string {
  if (status === "unresolved" || status === "interrupted") return "var(--ct-muted-fg)";
  return status === "error" ? "var(--ct-red)" : status === "pending" ? "var(--ct-yellow)" : "var(--ct-green)";
}

function statusName(status: ToolEvent["status"]): string {
  return status === "unresolved" ? "no result" : status;
}

function StatusGlyph({ status }: { status: ToolEvent["status"] }) {
  const style = { color: statusColor(status) };
  if (status === "pending") {
    return <LoaderCircle data-testid="chat-tool-stream-spinner" aria-hidden="true" size={12} className="shrink-0 motion-safe:animate-spin" style={style} />;
  }
  if (status === "unresolved") {
    return <Minus data-testid="chat-tool-stream-no-result" aria-hidden="true" size={12} className="shrink-0" style={style} />;
  }
  if (status === "interrupted") {
    return <Minus data-testid="chat-tool-stream-interrupted" aria-hidden="true" size={12} className="shrink-0" style={style} />;
  }
  return status === "error"
    ? <CircleX aria-hidden="true" size={12} className="shrink-0" style={style} />
    : <Check aria-hidden="true" size={12} className="shrink-0" style={style} />;
}

function actionLabel(member: ToolEvent): string {
  return toolStreamLabel(member) || member.summary || "Tool activity";
}

export function ToolActivityStream({ event, expanded, onToggle, renderAction }: {
  event: ToolEvent;
  expanded: boolean;
  onToggle: (next: boolean) => void;
  renderAction: RenderStreamAction;
}) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const regionId = useId();
  const stream = event.stream;
  if (!stream) return null;
  const count = stream.members.length;
  const countLabel = `${count} ${count === 1 ? "action" : "actions"}`;
  const miniView = stream.active && !expanded;

  const renderMember = (member: ToolEvent) => (
    <li key={member.id} data-testid="chat-tool-stream-member" data-member-id={member.id}>
      {member.id === selectedId
        ? renderAction(member, true, (next) => setSelectedId(next ? member.id : null))
        : (
          <button
            type="button"
            data-testid="chat-tool-stream-action"
            aria-expanded={false}
            aria-label={`Show ${actionLabel(member)}`}
            onClick={() => setSelectedId(member.id)}
            className="flex w-full min-w-0 items-center gap-2 rounded px-2 py-1 text-left font-mono text-[11.5px] hover:bg-[var(--ct-surface)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]"
            style={{ color: "var(--ct-fg)" }}
          >
            <StatusGlyph status={member.status} />
            <span className="min-w-0 truncate">{actionLabel(member)}</span>
            <ToolOutcomeFacts event={member} />
          </button>
        )}
    </li>
  );

  return (
    <section
      data-testid="chat-tool-stream"
      data-flow-active={stream.active ? "true" : "false"}
      data-tool-status={event.status}
      aria-label={`Tool activity, ${countLabel}`}
      className="mx-auto mb-1.5 w-full max-w-4xl min-w-0 overflow-hidden rounded-md border-l-2"
      style={{
        borderLeftColor: stream.active ? "var(--ct-accent)" : "var(--ct-border)",
        background: "color-mix(in srgb, var(--ct-surface) 60%, transparent)",
        maxHeight: miniView ? MINI_VIEW_MAX_PX : undefined,
      }}
    >
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls={regionId}
        aria-label={`${expanded ? "Collapse" : "Expand"} ${countLabel}`}
        onClick={() => onToggle(!expanded)}
        className="flex w-full min-w-0 items-center gap-2 px-3 py-1.5 text-left text-xs"
      >
        <span data-testid="chat-tool-stream-status" aria-label={statusName(event.status)} className="flex shrink-0">
          <StatusGlyph status={event.status} />
        </span>
        <span
          data-testid="chat-tool-stream-title"
          aria-live="polite"
          className="min-w-0 flex-1 truncate font-mono text-[12px] motion-safe:transition-opacity motion-reduce:transition-none"
          style={{ color: "var(--ct-fg)" }}
        >
          {event.summary || "Tool activity"}
        </span>
        <span className="shrink-0 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>{countLabel}</span>
        <ChevronRight aria-hidden="true" size={14} className={`shrink-0 ${expanded ? "rotate-90" : ""}`} style={{ color: "var(--ct-muted-fg)" }} />
      </button>
      {miniView && (
        <ol data-testid="chat-tool-stream-mini" aria-label="Recent actions" className="m-0 list-none px-3 pb-1.5 pl-8">
          {stream.members.slice(-MINI_VIEW_ROWS).map((member) => (
            <li key={member.id} className="flex min-w-0 items-center gap-1.5 font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
              <StatusGlyph status={member.status} />
              <span className="min-w-0 truncate">{actionLabel(member)}</span>
              <ToolOutcomeFacts event={member} />
            </li>
          ))}
        </ol>
      )}
      <div id={regionId} role="region" aria-label="Tool actions" hidden={!expanded} className="px-2 pb-1.5">
        {expanded && (
          <ul className="m-0 list-none p-0">
            {stream.entries.map((entry) => entry.kind === "action" ? renderMember(entry.event) : (
              <li key={entry.id} data-testid="chat-tool-stream-thought">
                <ThoughtLine text={entry.text} />
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
