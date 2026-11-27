import { toolCallSummaryLine, type ToolEvent } from "@codeterm/chat-core";
import { ChevronRight } from "lucide-react";
import { useId, useState } from "react";
import { CardChevron } from "../CompactCard";
import { RowMenuIsolate } from "../messageMenu";
import { ThoughtLine } from "./ThoughtLine";
import { StatusGlyph, statusName, type RenderStreamAction } from "./ToolActivityStream";
import { ToolOutcomeFacts } from "./ToolOutcomeFacts";
import { toolEventVisual } from "./toolVisualModel";

const OPEN_MEMBERS_BY_RUN = new Map<string, Set<string>>();
const MAX_TRACKED_RUNS = 512;

function memberKey(member: ToolEvent): string {
  return member.toolId?.trim() || member.id;
}

function runPersistenceKey(event: ToolEvent): string {
  const first = event.toolRun?.members[0];
  const anchor = first?.toolId?.trim() || first?.id || event.id;
  return `${event.message.sourceSessionId ?? ""}:${anchor}`;
}

function readOpenMembers(key: string): Set<string> {
  return new Set(OPEN_MEMBERS_BY_RUN.get(key) ?? []);
}

function writeOpenMembers(key: string, next: Set<string>): void {
  OPEN_MEMBERS_BY_RUN.delete(key);
  if (next.size > 0) {
    OPEN_MEMBERS_BY_RUN.set(key, next);
    if (OPEN_MEMBERS_BY_RUN.size > MAX_TRACKED_RUNS) {
      const oldest = OPEN_MEMBERS_BY_RUN.keys().next().value;
      if (oldest !== undefined) OPEN_MEMBERS_BY_RUN.delete(oldest);
    }
  }
}

function memberLabel(member: ToolEvent, ordinal: number): string {
  return toolCallSummaryLine(member) ?? `${member.message.toolName?.trim() || "Call"} #${ordinal}`;
}

export function ToolRunCard({ event, expanded, onToggle, renderAction }: {
  event: ToolEvent;
  expanded: boolean;
  onToggle: (next: boolean) => void;
  renderAction: RenderStreamAction;
}) {
  const toolRun = event.toolRun;
  const persistKey = runPersistenceKey(event);
  const [openMembers, setOpenMembers] = useState<Set<string>>(() => readOpenMembers(persistKey));
  const regionId = useId();
  if (!toolRun) return null;

  const setMemberOpen = (key: string, open: boolean) => {
    setOpenMembers((previous) => {
      const next = new Set(previous);
      if (open) next.add(key);
      else next.delete(key);
      writeOpenMembers(persistKey, next);
      return next;
    });
  };

  const count = toolRun.members.length;
  const heading = `${toolRun.toolName} × ${count}`;
  const visual = toolEventVisual(toolRun.members[0]);
  const Glyph = visual.glyph;
  const quietIcon = toolRun.kind === "read" || toolRun.kind === "search" || toolRun.kind === "generic" || toolRun.kind === "fetch";
  const iconColor = event.status === "error" || quietIcon ? "var(--ct-muted-fg)" : visual.color;

  const ordinalById = new Map<string, number>();
  toolRun.members.forEach((member, index) => ordinalById.set(member.id, index + 1));

  const renderMemberRow = (member: ToolEvent, testId: string) => {
    const key = memberKey(member);
    const ordinal = ordinalById.get(member.id) ?? 1;
    const summary = memberLabel(member, ordinal);
    const isOpen = openMembers.has(key);
    const isError = member.status === "error";
    return (
      <li
        key={member.id}
        data-testid={testId}
        data-member-id={member.id}
        data-member-status={member.status}
        className="min-w-0"
      >
        {isOpen
          ? renderAction(member, true, (next) => setMemberOpen(key, next))
          : (
            <button
              type="button"
              data-testid="chat-tool-run-member-toggle"
              aria-expanded={false}
              aria-label={`Show ${summary}`}
              onClick={() => setMemberOpen(key, true)}
              className="flex w-full min-w-0 items-center gap-2 rounded px-2 py-1 text-left font-mono text-[11.5px] transition-colors hover:bg-[var(--ct-surface)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]"
              style={{
                color: isError ? "var(--ct-red)" : "var(--ct-fg)",
                background: isError ? "color-mix(in srgb, var(--ct-red) 6%, transparent)" : undefined,
              }}
            >
              <StatusGlyph status={member.status} />
              <span className="shrink-0 text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>{`#${ordinal}`}</span>
              <span data-testid="chat-tool-run-member-summary" className="min-w-0 flex-1 truncate">{summary}</span>
              <ToolOutcomeFacts event={member} />
              <ChevronRight aria-hidden="true" size={12} className="shrink-0" style={{ color: "var(--ct-muted-fg)" }} />
            </button>
          )}
      </li>
    );
  };

  return (
    <RowMenuIsolate>
      <section
        data-testid="chat-tool-run"
        data-tool-status={event.status}
        data-tool-kind={toolRun.kind}
        data-expanded={expanded}
        aria-label={heading}
        className={`ct-card ct-command-inline mx-auto mb-1.5 w-full max-w-4xl min-w-0 overflow-hidden rounded-md${event.status === "error" ? " ct-card--error" : ""}`}
      >
        <div className="ct-command-inline__head flex w-full min-w-0 items-center gap-2 text-xs">
          <button
            type="button"
            data-testid="chat-tool-run-toggle"
            aria-expanded={expanded}
            aria-controls={regionId}
            aria-label={`${expanded ? "Collapse" : "Expand"} ${heading}`}
            onClick={() => onToggle(!expanded)}
            className="flex min-w-0 flex-1 items-center gap-2 text-left"
          >
            <span className="ct-card__icon" style={{ color: iconColor }}>
              <Glyph aria-hidden="true" size={14} strokeWidth={2} style={{ color: iconColor }} />
            </span>
            <span className="flex min-w-0 flex-1 items-baseline gap-1.5 overflow-hidden font-mono text-[12.5px]">
              <span data-testid="chat-tool-run-title" className="flex min-w-0 items-baseline font-medium" style={{ color: "var(--ct-fg)" }}>
                <span className="min-w-0 truncate">{toolRun.toolName}</span>
                <span className="shrink-0 whitespace-pre" style={{ color: "var(--ct-muted-fg)" }}>{` × ${count}`}</span>
              </span>
              {toolRun.runningCount > 0 && (
                <span data-testid="chat-tool-run-running" className="shrink-0 font-mono text-[10.5px]" style={{ color: "var(--ct-yellow)" }}>
                  {toolRun.runningCount > 1 ? `${toolRun.runningCount} running` : "running"}
                </span>
              )}
              {toolRun.failedCount > 0 && (
                <span
                  data-testid="chat-tool-run-failures"
                  className="shrink-0 rounded px-1.5 py-0.5 font-mono text-[10.5px] font-medium"
                  style={{
                    color: "var(--ct-red)",
                    background: "color-mix(in srgb, var(--ct-red) 12%, transparent)",
                  }}
                >
                  {`${toolRun.failedCount} failed`}
                </span>
              )}
            </span>
          </button>
          <span
            data-testid="chat-tool-run-status"
            role="status"
            aria-label={statusName(event.status)}
            className="ct-tool-status-icon flex w-4 shrink-0 items-center justify-center"
          >
            <StatusGlyph status={event.status} />
          </span>
          <CardChevron
            expanded={expanded}
            label={`${expanded ? "Collapse" : "Expand"} ${heading}`}
            controls={regionId}
            onToggle={() => onToggle(!expanded)}
          />
        </div>
        {!expanded && toolRun.errors.length > 0 && (
          <ul
            data-testid="chat-tool-run-errors"
            aria-label={`Failed calls in ${toolRun.toolName}`}
            className="m-0 flex list-none flex-col gap-0.5 pb-1.5 pl-6 pr-2 pt-0.5"
          >
            {toolRun.errors.map((member) => renderMemberRow(member, "chat-tool-run-error-member"))}
          </ul>
        )}
        <div
          id={regionId}
          role="region"
          aria-label={`${toolRun.toolName} calls`}
          hidden={!expanded}
          className="pb-1.5 pl-6 pr-2 pt-0.5"
        >
          {expanded && (
            <ul data-testid="chat-tool-run-list" className="m-0 flex list-none flex-col gap-0.5 p-0">
              {toolRun.entries.map((entry) => entry.kind === "action"
                ? renderMemberRow(entry.event, "chat-tool-run-member")
                : (
                  <li key={entry.id} data-testid="chat-tool-run-thought" className="px-2 py-0.5">
                    <ThoughtLine text={entry.text} />
                  </li>
                ))}
            </ul>
          )}
        </div>
      </section>
    </RowMenuIsolate>
  );
}
