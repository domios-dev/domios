import type { KeyboardEvent } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { TASK_STATUS_COLORS } from "../settings";
import type { PlanListRow, PlanListRowKind, PlanListVisibleRow } from "./buildPlanListTree";
import { isPlanListSectionRow } from "./buildPlanListTree";

const ENTITY_KINDS = new Set<PlanListRowKind>(["spec-root", "plan-root"]);
const TASK_KINDS = new Set<PlanListRowKind>(["task", "subtask"]);

const GLYPH: Record<string, string> = {
  done: "✓",
  progress: "◐",
  uncovered: "○",
};

function rowTestId(kind: PlanListRowKind): string {
  if (kind === "spec-root") return "plan-list-spec-root";
  if (kind === "plan-root") return "plan-list-plan-root";
  if (kind === "spec-section") return "plan-list-spec-section";
  if (kind === "plan-section") return "plan-list-plan-section";
  if (kind === "unsectioned") return "plan-list-unsectioned";
  if (kind === "subtask") return "plan-list-subtask";
  return "plan-list-task";
}

function StatusChip({ label, color }: { label: string; color: string | null }) {
  return (
    <span
      data-testid="plan-list-status"
      className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide"
      style={{
        color: color ?? "var(--ct-muted-fg)",
        background: color ? `color-mix(in srgb, ${color} 14%, transparent)` : "var(--ct-bg-active)",
        border: `1px solid ${color ? `color-mix(in srgb, ${color} 35%, transparent)` : "var(--ct-border)"}`,
      }}
    >
      {label}
    </span>
  );
}

export interface PlanListRowViewProps {
  entry: PlanListVisibleRow;
  sectionExpanded?: boolean;
  onToggleSection?: (sectionId: string) => void;
  onOpenRow?: (row: PlanListRow) => void;
}

export function PlanListRowView({ entry, sectionExpanded, onToggleSection, onOpenRow }: PlanListRowViewProps) {
  if (entry.type === "done-rollup") {
    const { sectionRow, doneCount } = entry;
    return (
      <button
        type="button"
        data-testid="plan-list-done-rollup"
        data-section-id={sectionRow.id}
        className="flex w-full min-w-0 cursor-pointer items-center gap-2 border-b px-3 py-1 text-left"
        style={{
          paddingLeft: 12 + sectionRow.depth * 16 + 16,
          borderColor: "color-mix(in srgb, var(--ct-border) 45%, transparent)",
          color: "var(--ct-muted-fg)",
          background: "transparent",
        }}
        onClick={() => onToggleSection?.(sectionRow.id)}
      >
        <span className="text-[10px] tabular-nums italic">
          {doneCount} done
        </span>
      </button>
    );
  }

  const row = entry.row;
  const isEntity = ENTITY_KINDS.has(row.kind);
  const isSection = isPlanListSectionRow(row);
  const isTask = TASK_KINDS.has(row.kind);
  const isDoneTask = isTask && row.status === "done";
  const statusColor =
    row.status && row.kind !== "plan-root" ? TASK_STATUS_COLORS[row.status as keyof typeof TASK_STATUS_COLORS] ?? null : null;
  const indent = row.depth * 16;
  const interactive = isTask || isEntity || isSection;
  const RowTag = interactive ? "button" : "div";

  const handleClick = () => {
    if (isSection) {
      onToggleSection?.(row.id);
      return;
    }
    if (isTask || isEntity) onOpenRow?.(row);
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    e.preventDefault();
    handleClick();
  };

  return (
    <RowTag
      type={interactive ? "button" : undefined}
      data-testid={rowTestId(row.kind)}
      data-row-kind={row.kind}
      data-plan-id={row.planId}
      data-task-id={row.taskId}
      data-section-expanded={isSection ? String(sectionExpanded ?? true) : undefined}
      className={`flex min-w-0 items-start gap-2 border-b px-3 text-left ${interactive ? "cursor-pointer" : ""} ${isDoneTask ? "py-1 opacity-70" : "py-1.5"}`}
      style={{
        paddingLeft: 12 + indent,
        borderColor: "color-mix(in srgb, var(--ct-border) 55%, transparent)",
        background: isEntity
          ? "color-mix(in srgb, var(--ct-titlebar-bg) 88%, var(--ct-bg))"
          : isSection
            ? "color-mix(in srgb, var(--ct-bg-elev, var(--ct-bg)) 92%, transparent)"
            : "transparent",
        color: "inherit",
      }}
      onClick={interactive ? handleClick : undefined}
      onKeyDown={interactive ? handleKeyDown : undefined}
    >
      {isSection && (
        <span
          data-testid="plan-list-collapse-toggle"
          aria-hidden
          className="shrink-0 pt-0.5"
          style={{ color: "var(--ct-muted-fg)" }}
        >
          {sectionExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </span>
      )}
      {isEntity && row.kicker && (
        <span
          data-testid="plan-list-kicker"
          className="shrink-0 pt-0.5 text-[10px] font-bold tracking-wider"
          style={{ color: "var(--ct-muted-fg)", minWidth: "2.5rem" }}
        >
          {row.kicker}
        </span>
      )}
      {isSection && row.sectionGlyph && (
        <span
          data-testid="plan-list-section-glyph"
          aria-hidden
          className="shrink-0 pt-0.5 text-[11px]"
          style={{ color: "var(--ct-muted-fg)", width: "1rem", textAlign: "center" }}
        >
          {GLYPH[row.sectionGlyph] ?? "·"}
        </span>
      )}
      {isTask && (
        <span aria-hidden className="shrink-0 pt-1 text-[10px]" style={{ color: "var(--ct-muted-fg)", width: "0.75rem" }}>
          {row.kind === "subtask" ? "↳" : "•"}
        </span>
      )}
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5">
          <span
            data-testid="plan-list-title"
            className="min-w-0 break-words"
            style={{
              fontSize: isEntity ? "0.88rem" : isSection ? "0.8rem" : isDoneTask ? "0.74rem" : "0.78rem",
              fontWeight: isEntity ? 700 : isSection ? 600 : isDoneTask ? 400 : 500,
              lineHeight: 1.35,
              color: isDoneTask ? "color-mix(in srgb, var(--ct-fg) 62%, transparent)" : "var(--ct-fg)",
            }}
          >
            {row.title}
          </span>
          {isSection && row.taskCount != null && row.taskCount > 0 && (
            <span data-testid="plan-list-task-count" className="text-[10px] tabular-nums" style={{ color: "var(--ct-muted-fg)" }}>
              {row.taskCount} task{row.taskCount === 1 ? "" : "s"}
            </span>
          )}
        </div>
        {(row.meta || row.focus) && isEntity && (
          <div className="flex min-w-0 flex-col gap-0.5">
            {row.meta && (
              <span data-testid="plan-list-meta" className="text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
                {row.meta}
              </span>
            )}
            {row.focus && (
              <span
                data-testid="plan-list-focus"
                className="break-words text-[11px] italic"
                style={{ color: "color-mix(in srgb, var(--ct-fg) 68%, transparent)" }}
              >
                {row.focus}
              </span>
            )}
          </div>
        )}
        {isTask && (row.paneLabel || row.historicalExecutorSummary || row.milestoneKind || row.focus) && (
          <div
            className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 text-[10px]"
            style={{ color: "var(--ct-muted-fg)", opacity: isDoneTask ? 0.85 : 1 }}
          >
            {row.paneLabel && (
              <span data-testid="plan-list-active-executor" className="font-medium" style={{ color: "var(--ct-fg)" }}>
                {row.paneLabel}
              </span>
            )}
            {row.historicalExecutorSummary && !row.paneLabel && (
              <span data-testid="plan-list-historical-executor" style={{ opacity: 0.82 }}>
                {row.historicalExecutorSummary}
              </span>
            )}
            {row.focus && (
              <span data-testid="plan-list-task-focus" className="min-w-0 break-words italic">
                {row.focus}
              </span>
            )}
            {row.milestoneKind && (
              <span data-testid="plan-list-milestone">
                {row.milestoneAuthor ? `${row.milestoneAuthor} · ` : ""}
                {row.milestoneKind}
              </span>
            )}
          </div>
        )}
      </div>
      {row.statusLabel && isTask && <StatusChip label={row.statusLabel} color={statusColor} />}
      {row.kind === "plan-root" && row.status && (
        <StatusChip label={row.status} color={row.status === "active" ? "rgba(82, 180, 145, 0.9)" : "var(--ct-muted-fg)"} />
      )}
    </RowTag>
  );
}
