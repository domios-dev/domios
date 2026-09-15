import type { ToolEvent } from "@codeterm/chat-core";
import { FileCode, Terminal } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { commandRowLabel, type CommandRow } from "../codeterm/commandShape";
import { describeAction } from "../codeterm/templates";
import { actionVisual } from "./CodetermActionCard";
import { toolKindVisual, type ToolKindVisual } from "./toolKindVisual";

export const COMMAND_TREE_CAP = 3;

const OPEN_SEGMENT_BY_CALL = new Map<string, number>();

/** One inline line under a call: a chain segment, a diff hunk, its first output line, or an assigned variable. */
export interface Branch {
  kind: "segment" | "hunk" | "output" | "assignment";
  lead: string;
  object?: string;
  glyph?: ToolKindVisual["glyph"];
  color?: string;
  role?: CommandRow["role"];
  dim?: boolean;
  marker?: string;
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

function segmentBranch(row: CommandRow): Branch {
  if (row.role === "codeterm") {
    const { lead, object } = describeAction(row.invocation);
    const visual = actionVisual(row.invocation);
    return { kind: "segment", role: row.role, glyph: visual.glyph, color: visual.color, lead, object };
  }
  if (row.role === "script") {
    const { language, lineCount } = row.script;
    return { kind: "segment", role: row.role, glyph: FileCode, color: "var(--ct-cyan)", lead: language, object: lineLabel(lineCount) };
  }
  const { assignment } = row.segment;
  return {
    kind: "segment", role: row.role, glyph: Terminal, color: toolKindVisual.command.color, lead: commandRowLabel(row),
    object: assignment ? `→ ${assignment}` : undefined, dim: Boolean(assignment),
  };
}

const LINE_CLASS = "flex w-full min-w-0 items-center gap-1.5 rounded px-1 text-left leading-[18px] hover:bg-[var(--ct-surface)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]";
const QUIET = "var(--ct-muted-fg)";

/** Compact inline branches under a call, capped with "+N more"; at most one detail opens inline. */
export function BranchList({ branches, cap, openIndex, onBranch, onMore, renderDetail, label }: {
  branches: Branch[];
  cap?: number;
  openIndex: number | null;
  onBranch: (index: number) => void;
  onMore?: () => void;
  renderDetail?: (index: number) => ReactNode;
  label: string;
}) {
  const [showAll, setShowAll] = useState(false);
  const limit = showAll ? undefined : cap;
  const visible = limit != null && branches.length > limit ? limit : branches.length;
  const hidden = branches.length - visible;
  const branchColor = { color: QUIET, opacity: 0.6 };

  return (
    <ol data-testid="chat-command-tree" aria-label={label} className="m-0 min-w-0 list-none py-0.5 pl-4 pr-2 font-mono text-[11.5px]">
      {branches.slice(0, visible).map((branch, index) => {
        const open = renderDetail != null && openIndex === index;
        const Glyph = branch.glyph;
        const name = branch.object ? `${branch.lead} ${branch.object}` : branch.lead;
        return (
          <li key={index} data-testid="chat-command-tree-line" data-branch-kind={branch.kind} data-row-role={branch.role} className="min-w-0">
            <button
              type="button"
              aria-expanded={renderDetail ? open : undefined}
              aria-label={`${open ? "Hide" : "Show"} ${name}`}
              onClick={() => onBranch(index)}
              className={LINE_CLASS}
            >
              <span aria-hidden="true" className="shrink-0" style={branchColor}>{index === branches.length - 1 ? "└" : "├"}</span>
              {Glyph && <Glyph aria-hidden="true" size={12} className="shrink-0" style={{ color: branch.color }} />}
              <span
                data-testid="chat-command-tree-lead"
                className={branch.kind === "output" ? "min-w-0 truncate" : "shrink-0"}
                style={{ color: branch.color ?? QUIET }}
              >
                {branch.lead}
              </span>
              {branch.object && (
                <span data-testid="chat-command-tree-object" className="min-w-0 truncate" style={{ color: branch.dim ? QUIET : "var(--ct-fg)" }}>{branch.object}</span>
              )}
              {branch.marker && (
                <span data-testid="chat-command-tree-marker" className="shrink-0 text-[10px]" style={{ color: "var(--ct-red)", opacity: 0.75 }}>{branch.marker}</span>
              )}
            </button>
            {open && <div data-testid="chat-command-tree-detail" className="mb-1 ml-[5ch] mt-0.5 min-w-0">{renderDetail?.(index)}</div>}
          </li>
        );
      })}
      {hidden > 0 && (
        <li className="min-w-0">
          <button
            type="button"
            data-testid="chat-command-tree-more"
            aria-label={`Show ${hidden} more`}
            onClick={() => (onMore ? onMore() : setShowAll(true))}
            className={LINE_CLASS}
            style={{ color: QUIET }}
          >
            <span aria-hidden="true" style={branchColor}>└</span>
            <span>{`+${hidden} more`}</span>
          </button>
        </li>
      )}
    </ol>
  );
}

/** One compact tree line per shell segment of a chained call. */
export function CommandTree({ rows, cap, openIndex, onLine, onMore, renderDetail, label = "Commands" }: {
  rows: CommandRow[];
  cap?: number;
  openIndex: number | null;
  onLine: (index: number) => void;
  onMore?: () => void;
  renderDetail?: (index: number) => ReactNode;
  label?: string;
}) {
  const branches = useMemo(() => rows.map((row) => segmentBranch(row)), [rows]);
  return <BranchList branches={branches} cap={cap} openIndex={openIndex} onBranch={onLine} onMore={onMore} renderDetail={renderDetail} label={label} />;
}
