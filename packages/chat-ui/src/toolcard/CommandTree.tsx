import type { ShellOperator, ToolEvent } from "@codeterm/chat-core";
import { FileCode, Terminal } from "lucide-react";
import { useMemo, type ReactNode } from "react";
import { commandRowLabel, operatorSymbol, type CommandRow } from "../codeterm/commandShape";
import { describeAction } from "../codeterm/templates";
import { actionVisual } from "./CodetermActionCard";
import { toolKindVisual, type ToolKindVisual } from "./toolKindVisual";

export const COMMAND_TREE_CAP = 3;

const OPEN_SEGMENT_BY_CALL = new Map<string, number>();
const OPERATOR_NAMES: Record<ShellOperator, string> = {
  pipe: "output piped into",
  and: "then, when it succeeds",
  or: "then, when it fails",
  then: "then",
};

interface TreeLine {
  role: CommandRow["role"];
  visual: ToolKindVisual;
  lead: string;
  object?: string;
  /** Operator joining the previous segment to this one; null on the first line. */
  operator: ShellOperator | null;
}

export function commandCallKey(event: ToolEvent): string {
  return `${event.message.sourceSessionId ?? ""}:${event.toolId?.trim() || event.id}`;
}

export function openSegmentOf(callKey: string): number | null {
  return OPEN_SEGMENT_BY_CALL.get(callKey) ?? null;
}

export function rememberOpenSegment(callKey: string, index: number | null): void {
  if (index == null) OPEN_SEGMENT_BY_CALL.delete(callKey);
  else OPEN_SEGMENT_BY_CALL.set(callKey, index);
}

function lineLabel(count: number): string {
  return `${count} ${count === 1 ? "line" : "lines"}`;
}

function treeLine(row: CommandRow, previous: CommandRow | undefined): TreeLine {
  const operator = previous ? previous.segment.operator ?? "then" : null;
  if (row.role === "codeterm") {
    const { lead, object } = describeAction(row.invocation);
    return { role: row.role, visual: actionVisual(row.invocation), lead, object, operator };
  }
  if (row.role === "script") {
    const { language, lineCount } = row.script;
    return { role: row.role, visual: { glyph: FileCode, color: "var(--ct-cyan)", label: language }, lead: language, object: lineLabel(lineCount), operator };
  }
  return { role: row.role, visual: { glyph: Terminal, color: toolKindVisual.command.color, label: "command" }, lead: commandRowLabel(row), operator };
}

const LINE_CLASS = "flex w-full min-w-0 items-center gap-1.5 rounded px-1 text-left leading-[18px] hover:bg-[var(--ct-surface)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]";

/** One compact line per shell segment of a chained call, with its joining operator inline. */
export function CommandTree({ rows, cap, openIndex, onLine, onMore, renderDetail, label = "Commands" }: {
  rows: CommandRow[];
  cap?: number;
  openIndex: number | null;
  onLine: (index: number) => void;
  onMore?: () => void;
  renderDetail?: (index: number) => ReactNode;
  label?: string;
}) {
  const lines = useMemo(() => rows.map((row, index) => treeLine(row, rows[index - 1])), [rows]);
  const visible = cap != null && lines.length > cap ? cap : lines.length;
  const hidden = lines.length - visible;
  const branchColor = { color: "var(--ct-muted-fg)", opacity: 0.6 };

  return (
    <ol data-testid="chat-command-tree" aria-label={label} className="m-0 min-w-0 list-none py-0.5 pl-6 pr-2 font-mono text-[11.5px]">
      {lines.slice(0, visible).map((line, index) => {
        const open = renderDetail != null && openIndex === index;
        const Glyph = line.visual.glyph;
        const name = line.object ? `${line.lead} ${line.object}` : line.lead;
        return (
          <li key={index} data-testid="chat-command-tree-line" data-row-role={line.role} className="min-w-0">
            <button
              type="button"
              aria-expanded={renderDetail ? open : undefined}
              aria-label={`${open ? "Hide" : "Show"} ${name}`}
              onClick={() => onLine(index)}
              className={LINE_CLASS}
            >
              <span aria-hidden="true" className="shrink-0" style={branchColor}>{index === lines.length - 1 ? "└" : "├"}</span>
              {line.operator ? (
                <span data-testid="chat-command-tree-operator" data-operator={line.operator} className="w-[2ch] shrink-0 text-[10px]" style={{ color: "var(--ct-muted-fg)" }}>
                  <span aria-hidden="true">{operatorSymbol(line.operator)}</span>
                  <span className="sr-only">{OPERATOR_NAMES[line.operator]}</span>
                </span>
              ) : <span aria-hidden="true" className="w-[2ch] shrink-0" />}
              <Glyph aria-hidden="true" size={12} className="shrink-0" style={{ color: line.visual.color }} />
              <span data-testid="chat-command-tree-lead" className="shrink-0" style={{ color: line.visual.color }}>{line.lead}</span>
              {line.object && <span data-testid="chat-command-tree-object" className="min-w-0 truncate" style={{ color: "var(--ct-fg)" }}>{line.object}</span>}
            </button>
            {open && <div data-testid="chat-command-tree-detail" className="mb-1 ml-[5ch] mt-0.5 min-w-0">{renderDetail?.(index)}</div>}
          </li>
        );
      })}
      {hidden > 0 && (
        <li className="min-w-0">
          <button type="button" data-testid="chat-command-tree-more" aria-label={`Show ${hidden} more commands`} onClick={onMore} disabled={!onMore} className={LINE_CLASS} style={{ color: "var(--ct-muted-fg)" }}>
            <span aria-hidden="true" style={branchColor}>└</span>
            <span className="w-[2ch] shrink-0" aria-hidden="true" />
            <span>{`+${hidden} more`}</span>
          </button>
        </li>
      )}
    </ol>
  );
}
