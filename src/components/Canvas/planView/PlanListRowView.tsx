import type { KeyboardEvent } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { TASK_STATUS_COLORS } from "../settings";
import type { PlanListRow, PlanListRowKind, PlanListVisibleRow } from "./buildPlanListTree";
import { isPlanListSectionRow } from "./buildPlanListTree";
import { formatRelativeAge } from "./planListModel";

const TASK_KINDS = new Set<PlanListRowKind>(["task", "subtask"]);
const STATUS_WORD: Partial<Record<string, string>> = { blocked: "blocked", in_review: "review" };
const TASK_INDENT_STEP = 16;

const GLYPH: Record<string, string> = {
  done: "✓",
  progress: "◐",
  uncovered: "○",
};

function rowTestId(kind: PlanListRowKind): string {
  if (kind === "plan-section") return "plan-list-plan-section";
  if (kind === "unsectioned") return "plan-list-unsectioned";
  if (kind === "subtask") return "plan-list-subtask";
  return "plan-list-task";
}

export interface PlanListRowViewProps {
  entry: PlanListVisibleRow;
  now?: number;
  sectionExpanded?: boolean;
  onToggleSection?: (sectionId: string) => void;
  onOpenRow?: (row: PlanListRow) => void;
}

export function PlanListRowView({ entry, now, sectionExpanded, onToggleSection, onOpenRow }: PlanListRowViewProps) {
  if (entry.type === "done-rollup") {
    const { sectionRow, doneCount } = entry;
    return (
      <button
        type="button"
        data-testid="plan-list-done-rollup"
        data-section-id={sectionRow.id}
        className="plan-list-rollup"
        onClick={() => onToggleSection?.(sectionRow.id)}
      >
        {doneCount} done
      </button>
    );
  }

  const row = entry.row;
  const isSection = isPlanListSectionRow(row);

  if (isSection) {
    return (
      <button
        type="button"
        data-testid={rowTestId(row.kind)}
        data-row-kind={row.kind}
        data-section-expanded={String(sectionExpanded ?? true)}
        className="plan-list-group"
        onClick={() => onToggleSection?.(row.id)}
      >
        <span data-testid="plan-list-collapse-toggle" aria-hidden className="plan-list-group-caret">
          {sectionExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </span>
        {row.sectionGlyph && (
          <span data-testid="plan-list-section-glyph" aria-hidden className="plan-list-group-glyph">
            {GLYPH[row.sectionGlyph] ?? "·"}
          </span>
        )}
        <span data-testid="plan-list-title" className="plan-list-group-title">
          {row.title}
        </span>
        {row.taskCount != null && row.taskCount > 0 && (
          <span data-testid="plan-list-task-count" className="plan-list-group-progress">
            {row.doneCount ?? 0}/{row.taskCount}
          </span>
        )}
      </button>
    );
  }

  const isTask = TASK_KINDS.has(row.kind);
  const isDoneTask = isTask && row.status === "done";
  const dotColor = row.status ? TASK_STATUS_COLORS[row.status] ?? "var(--ct-muted-fg)" : "var(--ct-muted-fg)";
  const statusWord = row.status ? STATUS_WORD[row.status] : undefined;
  const executor = row.paneLabel ?? row.historicalExecutorSummary ?? "";
  const age = formatRelativeAge(row.activityTs, now ?? 0);
  const titleIndent = Math.max(0, row.depth - 2) * TASK_INDENT_STEP;

  const handleClick = () => onOpenRow?.(row);
  const handleKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    e.preventDefault();
    handleClick();
  };

  return (
    <button
      type="button"
      data-testid={rowTestId(row.kind)}
      data-row-kind={row.kind}
      data-plan-id={row.planId}
      data-task-id={row.taskId}
      className={`plan-list-row${isDoneTask ? " plan-list-row-done" : ""}`}
      onClick={handleClick}
      onKeyDown={handleKeyDown}
      title={row.focus || row.title}
    >
      <span
        data-testid="plan-list-status-dot"
        aria-hidden
        className="plan-list-dot"
        style={{ background: dotColor }}
      />
      <span className="plan-list-title-cell" style={{ paddingLeft: titleIndent }}>
        <span data-testid="plan-list-title" className="plan-list-title">
          {row.title}
        </span>
        {statusWord && (
          <span data-testid="plan-list-status" className="plan-list-status-word">
            {statusWord}
          </span>
        )}
      </span>
      <span
        data-testid={row.paneLabel ? "plan-list-active-executor" : "plan-list-historical-executor"}
        className={`plan-list-exec${row.paneLabel ? " plan-list-exec-active" : ""}`}
      >
        {executor}
      </span>
      <span data-testid="plan-list-age" className="plan-list-age">
        {age}
      </span>
    </button>
  );
}
