import { toolCommand, type ShellOperator, type ShellScript, type ToolEvent } from "@codeterm/chat-core";
import { ChevronRight, FileCode, Terminal } from "lucide-react";
import { useId, useState } from "react";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { commandGroupSummary, operatorSymbol, type CommandRow } from "../codeterm/commandShape";
import type { OpenPlan, ResolvedChatPlanRef } from "../codeterm/LinkedText";
import { RawCommandDisclosure } from "../codeterm/RawCommandDisclosure";
import type { PeerNameResolver, TabNameResolver } from "../codeterm/templates";
import { DIFF_HIGHLIGHT_MAX_LINES } from "../MarkdownBody";
import { SYNTAX_HIGHLIGHT_ROOT_CLASS, themeAwarePrismStyle } from "../syntaxTheme";
import { ToolOutputContent } from "../ToolOutputContent";
import { CodetermActionCard } from "./CodetermActionCard";
import { StatusGlyph, statusName } from "./ToolActivityStream";
import { ToolOutcomeFacts } from "./ToolOutcomeFacts";
import { toolKindVisual } from "./toolKindVisual";

type PlanFocusKind = "plan" | "task" | "section";

interface CodetermRowProps {
  resolveTabName?: TabNameResolver;
  resolvePeerName?: PeerNameResolver;
  onFocusTab?: (tabRef: string) => void;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: PlanFocusKind) => ResolvedChatPlanRef | null;
  fontScale?: number;
}

interface CommandGroupCardProps extends CodetermRowProps {
  event: ToolEvent;
  rows: CommandRow[];
  expanded: boolean;
  onToggle: (next: boolean) => void;
}

const OPEN_ROW_BY_CALL = new Map<string, number>();
const OPERATOR_NAMES: Record<ShellOperator, string> = {
  pipe: "output piped into",
  and: "then, when it succeeds",
  or: "then, when it fails",
  then: "then",
};
const ROW_BUTTON_CLASS = "flex w-full min-w-0 items-center gap-2 rounded px-2 py-1 text-left font-mono text-[11.5px] hover:bg-[var(--ct-surface)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]";

function firstLine(text: string): string {
  return text.split(/\r?\n/, 1)[0];
}

function lineLabel(count: number): string {
  return `${count} ${count === 1 ? "line" : "lines"}`;
}

function OperatorConnector({ operator }: { operator: ShellOperator }) {
  return (
    <div data-testid="chat-command-group-operator" data-operator={operator} className="flex items-center gap-2 py-px pl-[13px] font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>
      <span aria-hidden="true" className="h-3 w-px" style={{ background: "var(--ct-border)" }} />
      <span aria-hidden="true">{operatorSymbol(operator)}</span>
      <span className="sr-only">{OPERATOR_NAMES[operator]}</span>
    </div>
  );
}

function ScriptBody({ script }: { script: ShellScript }) {
  const plain = script.language === "text" || script.lineCount > DIFF_HIGHLIGHT_MAX_LINES;
  return (
    <div
      data-testid="chat-command-script-body"
      data-language={script.language}
      className={`${SYNTAX_HIGHLIGHT_ROOT_CLASS} ml-7 mt-0.5 max-h-72 overflow-auto rounded border px-2 py-1 font-mono text-[11.5px]`}
      style={{ borderColor: "var(--ct-border)", background: "var(--ct-surface)" }}
    >
      {plain ? <pre className="m-0 whitespace-pre" style={{ color: "var(--ct-fg)" }}>{script.body}</pre> : (
        <SyntaxHighlighter
          language={script.language}
          style={themeAwarePrismStyle}
          useInlineStyles={false}
          PreTag="pre"
          customStyle={{ margin: 0, padding: 0, background: "transparent", fontSize: "inherit", lineHeight: "inherit" }}
          codeTagProps={{ style: { fontFamily: "inherit" } }}
        >
          {script.body}
        </SyntaxHighlighter>
      )}
    </div>
  );
}

function ScriptRow({ row, open, onOpen }: { row: Extract<CommandRow, { role: "script" }>; open: boolean; onOpen: (next: boolean) => void }) {
  const { language, lineCount } = row.script;
  return (
    <>
      <button
        type="button"
        data-testid="chat-command-group-script"
        data-language={language}
        aria-expanded={open}
        aria-label={`${open ? "Hide" : "Show"} ${language} script, ${lineLabel(lineCount)}`}
        onClick={() => onOpen(!open)}
        className={ROW_BUTTON_CLASS}
      >
        <FileCode aria-hidden="true" size={12} className="shrink-0" style={{ color: "var(--ct-cyan)" }} />
        <span className="shrink-0 font-semibold" style={{ color: "var(--ct-cyan)" }}>{language}</span>
        <span className="min-w-0 flex-1 truncate" style={{ color: "var(--ct-fg)" }}>{firstLine(row.segment.text)}</span>
        <span className="shrink-0 text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>{lineLabel(lineCount)}</span>
      </button>
      {open && <ScriptBody script={row.script} />}
    </>
  );
}

function PlainRow({ row, open, onOpen }: { row: CommandRow; open: boolean; onOpen: (next: boolean) => void }) {
  return (
    <>
      <button
        type="button"
        data-testid="chat-command-group-command"
        aria-expanded={open}
        aria-label={`${open ? "Hide" : "Show"} command ${firstLine(row.segment.text)}`}
        onClick={() => onOpen(!open)}
        className={ROW_BUTTON_CLASS}
      >
        <Terminal aria-hidden="true" size={12} className="shrink-0" style={{ color: toolKindVisual.command.color }} />
        <span className="min-w-0 truncate" style={{ color: "var(--ct-fg)" }}>{firstLine(row.segment.text)}</span>
      </button>
      {open && (
        <pre data-testid="chat-command-group-segment-text" className="m-0 ml-7 max-h-48 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
          {row.segment.text}
        </pre>
      )}
    </>
  );
}

/** One command call made of several shell segments: a row per segment, one outcome for the whole call. */
export function CommandGroupCard({ event, rows, expanded, onToggle, ...codeterm }: CommandGroupCardProps) {
  const callKey = `${event.message.sourceSessionId ?? ""}:${event.toolId?.trim() || event.id}`;
  const [openRow, setOpenRowState] = useState<number | null>(() => OPEN_ROW_BY_CALL.get(callKey) ?? null);
  const regionId = useId();
  const setOpenRow = (index: number, next: boolean) => {
    if (next) OPEN_ROW_BY_CALL.set(callKey, index);
    else OPEN_ROW_BY_CALL.delete(callKey);
    setOpenRowState(next ? index : null);
  };
  const title = event.description || commandGroupSummary(rows);
  const countLabel = `${rows.length} commands`;
  const failed = event.status === "error";

  return (
    <section
      data-testid="chat-command-group"
      data-tool-status={event.status}
      aria-label={`Command, ${countLabel}`}
      className="mx-auto mb-1.5 w-full max-w-4xl min-w-0 overflow-hidden rounded-md border-l-2"
      style={{
        borderLeftColor: failed ? "var(--ct-red)" : event.status === "pending" ? "var(--ct-accent)" : "var(--ct-border)",
        background: "color-mix(in srgb, var(--ct-surface) 60%, transparent)",
      }}
    >
      <button
        type="button"
        data-testid="chat-command-group-toggle"
        aria-expanded={expanded}
        aria-controls={regionId}
        aria-label={`${expanded ? "Hide" : "Show"} output of ${title}`}
        onClick={() => onToggle(!expanded)}
        className="flex w-full min-w-0 items-center gap-2 px-3 py-1.5 text-left text-xs"
      >
        <span data-testid="chat-command-group-status" aria-label={statusName(event.status)} className="flex shrink-0">
          <StatusGlyph status={event.status} />
        </span>
        <span data-testid="chat-command-group-title" title={title} className="min-w-0 flex-1 truncate font-mono text-[12px]" style={{ color: "var(--ct-fg)" }}>
          {title}
        </span>
        <ToolOutcomeFacts event={event} />
        <span className="shrink-0 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>{countLabel}</span>
        <ChevronRight aria-hidden="true" size={14} className={`shrink-0 ${expanded ? "rotate-90" : ""}`} style={{ color: "var(--ct-muted-fg)" }} />
      </button>
      <ol data-testid="chat-command-group-rows" aria-label="Commands" className="m-0 list-none px-2 pb-1.5">
        {rows.map((row, index) => {
          const open = openRow === index;
          const onOpen = (next: boolean) => setOpenRow(index, next);
          const member: ToolEvent = {
            id: `${event.id}:segment:${index}`,
            phase: "call",
            status: event.status,
            kind: row.role === "codeterm" ? "codeterm" : "command",
            summary: row.segment.text,
            detail: row.segment.text,
            message: event.message,
          };
          return (
            <li key={index} data-testid="chat-command-group-row" data-row-role={row.role} className="min-w-0">
              {index > 0 && <OperatorConnector operator={rows[index - 1].segment.operator ?? "then"} />}
              {row.role === "codeterm" ? (
                <CodetermActionCard event={member} invocation={row.invocation} expanded={open} onToggle={() => onOpen(!open)} {...codeterm} />
              ) : row.role === "script" ? (
                <ScriptRow row={row} open={open} onOpen={onOpen} />
              ) : (
                <PlainRow row={row} open={open} onOpen={onOpen} />
              )}
            </li>
          );
        })}
      </ol>
      <div id={regionId} role="region" aria-label="Command output" hidden={!expanded} className="px-3 pb-2">
        {expanded && (
          <RawCommandDisclosure raw={toolCommand(event).command}>
            {event.status === "pending"
              ? <div data-testid="toolcard-spinner" className="font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>Running…</div>
              : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
          </RawCommandDisclosure>
        )}
      </div>
    </section>
  );
}
