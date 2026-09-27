import type { KeyboardEvent } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { TASK_STATUS_COLORS } from "../settings";
import type { PlanListRow, PlanListRowKind, PlanListVisibleRow } from "./buildPlanListTree";
import { isPlanListSectionRow } from "./buildPlanListTree";
import { formatRelativeAge } from "./planListModel";
import { DONE_COLUMN_COLLAPSED_LIMIT } from "./boardModel";
import { usePlanTaskActivate } from "./planTaskInteraction";
import { TaskExecutionBadges } from "./TaskExecutionBadges";
import { WorkingOnRow } from "./WorkingOnRowView";
import { TaskStatus } from "../types";

const TASK_KINDS = new Set<PlanListRowKind>(["task", "subtask"]);
const TASK_INDENT_STEP = 16;

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
  onToggleDoneFilter?: () => void;
  onOpenRow?: (row: PlanListRow) => void;
  onOpenBelonging?: (row: PlanListRow) => void;
}

export function PlanListRowView({
  entry,
  now,
  sectionExpanded,
  onToggleSection,
  onToggleDoneFilter,
  onOpenRow,
  onOpenBelonging,
}: PlanListRowViewProps) {
  if (entry.type === "done-filter-toggle") {
    return (
      <button
        type="button"
        data-testid="plan-list-done-filter-toggle"
        className="plan-list-rollup"
        onClick={() => onToggleDoneFilter?.()}
      >
        {entry.expanded
          ? `Show recent ${DONE_COLUMN_COLLAPSED_LIMIT}`
          : `Show all ${entry.totalCount}`}
      </button>
    );
  }

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
  const isPausedTask = isTask && row.status === TaskStatus.Paused;
  const dotColor = row.status ? TASK_STATUS_COLORS[row.status] ?? "var(--ct-muted-fg)" : "var(--ct-muted-fg)";
  const executor = row.tabLabel ?? row.historicalExecutorSummary ?? "";
  const age = formatRelativeAge(row.activityTs, now ?? 0);
  const titleIndent = Math.max(0, row.depth - 2) * TASK_INDENT_STEP;
  const chipTitle = onOpenBelonging
    ? `Open plan document for ${row.sectionTitle ?? "Unsectioned"}`
    : `Belongs to ${row.sectionTitle ?? "Unsectioned"}`;
  const rowTitle = isPausedTask && row.pauseReason
    ? `Paused: ${row.pauseReason}`
    : row.focus || row.title;

  const activate = () => onOpenRow?.(row);
  const openBelonging = () => onOpenBelonging?.(row);
  const { onPointerDown, onPointerMove, onClick } = usePlanTaskActivate(activate);
  const handleKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    e.preventDefault();
    activate();
  };
  const handleChipKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    e.stopPropagation();
    e.preventDefault();
    openBelonging();
  };

  return (
    <div
      role="button"
      tabIndex={0}
      data-testid={rowTestId(row.kind)}
      data-row-kind={row.kind}
      data-plan-id={row.planId}
      data-task-id={row.taskId}
      className={`plan-list-row plan-task-selectable${isDoneTask ? " plan-list-row-done" : ""}${isPausedTask ? " plan-list-row-paused" : ""}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onClick={onClick}
      onKeyDown={handleKeyDown}
      title={rowTitle}
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
        {row.unplannedLabel && (
          <span data-testid="plan-list-unplanned-badge" className="plan-list-unplanned-badge">
            {row.unplannedLabel}
          </span>
        )}
        <TaskExecutionBadges loop={row.loopBadge} competition={row.competeBadge} />
        {isPausedTask && (
          <span
            data-testid="plan-list-paused-chip"
            className="plan-list-paused-chip"
            title={rowTitle}
          >
            paused
          </span>
        )}
        <button
          type="button"
          data-testid="plan-list-belonging-chip"
          className="plan-list-belonging-chip"
          title={chipTitle}
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation();
            openBelonging();
          }}
          onKeyDown={handleChipKeyDown}
        >
          {row.sectionTitle ?? "Unsectioned"}
        </button>
      </span>
      {row.workingOn ? (
        <span className="plan-list-exec plan-list-exec-working">
          <WorkingOnRow model={row.workingOn} variant="list" />
        </span>
      ) : (
        <span
          data-testid={row.tabLabel ? "plan-list-active-executor" : "plan-list-historical-executor"}
          className={`plan-list-exec${row.tabLabel ? " plan-list-exec-active" : ""}`}
        >
          {executor}
        </span>
      )}
      <span data-testid="plan-list-age" className="plan-list-age">
        {age}
      </span>
    </div>
  );
}
