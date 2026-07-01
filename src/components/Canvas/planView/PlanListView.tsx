import { useEffect, useMemo, useState } from "react";
import { X } from "lucide-react";
import { filterPlansForCanvas } from "../planPaneMatch";
import { fetchSpecSections } from "../plansApi";
import { fixtureSpecSectionsByPlan } from "../perfFixture";
import { TASK_STATUS_COLORS } from "../settings";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import type { Section } from "../types";
import { buildPlanListForest, type PlanListRow, type PlanListRowKind } from "./buildPlanListTree";

const ENTITY_KINDS = new Set<PlanListRowKind>(["spec-root", "plan-root"]);
const SECTION_KINDS = new Set<PlanListRowKind>(["spec-section", "plan-section", "unsectioned"]);
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

function PlanListRowView({ row }: { row: PlanListRow }) {
  const isEntity = ENTITY_KINDS.has(row.kind);
  const isSection = SECTION_KINDS.has(row.kind);
  const isTask = TASK_KINDS.has(row.kind);
  const statusColor =
    row.status && row.kind !== "plan-root" ? TASK_STATUS_COLORS[row.status as keyof typeof TASK_STATUS_COLORS] ?? null : null;
  const indent = row.depth * 16;

  return (
    <div
      data-testid={rowTestId(row.kind)}
      data-row-kind={row.kind}
      data-plan-id={row.planId}
      data-task-id={row.taskId}
      className="flex min-w-0 items-start gap-2 border-b px-3 py-1.5"
      style={{
        paddingLeft: 12 + indent,
        borderColor: "color-mix(in srgb, var(--ct-border) 55%, transparent)",
        background: isEntity
          ? "color-mix(in srgb, var(--ct-titlebar-bg) 88%, var(--ct-bg))"
          : isSection
            ? "color-mix(in srgb, var(--ct-bg-elev, var(--ct-bg)) 92%, transparent)"
            : "transparent",
      }}
    >
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
              fontSize: isEntity ? "0.88rem" : isSection ? "0.8rem" : "0.78rem",
              fontWeight: isEntity ? 700 : isSection ? 600 : 500,
              lineHeight: 1.35,
              color: "var(--ct-fg)",
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
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 text-[10px]" style={{ color: "var(--ct-muted-fg)" }}>
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
    </div>
  );
}

export function PlanListView({
  mode,
  plans,
  loading = false,
  error = null,
  planId,
  orchestratorPaneId,
  panes = {},
  api,
  onClose,
}: PlanViewSurfaceProps) {
  const filteredPlans = useMemo(
    () => filterPlansForCanvas(plans, mode, { planId, orchestratorPaneId, panes }),
    [plans, mode, planId, orchestratorPaneId, panes],
  );
  const fixtureSpecSections = useMemo(() => fixtureSpecSectionsByPlan(filteredPlans), [filteredPlans]);
  const [fetchedSpecSections, setFetchedSpecSections] = useState<Map<string, Section[]>>(new Map());
  useEffect(() => {
    if (!api) return;
    let active = true;
    const targets = filteredPlans.filter((p) => p.primary_spec_id && !fixtureSpecSections.has(p.id));
    if (!targets.length) return;
    void Promise.all(
      targets.map((p) =>
        fetchSpecSections(api, p.primary_spec_id as string)
          .then((sections) => [p.id, sections] as const)
          .catch(() => [p.id, [] as Section[]] as const),
      ),
    ).then((pairs) => {
      if (active) setFetchedSpecSections(new Map(pairs));
    });
    return () => {
      active = false;
    };
  }, [api, filteredPlans, fixtureSpecSections]);

  const specSectionsByPlan = useMemo(() => {
    if (!fixtureSpecSections.size) return fetchedSpecSections;
    const merged = new Map(fetchedSpecSections);
    for (const [k, v] of fixtureSpecSections) merged.set(k, v);
    return merged;
  }, [fetchedSpecSections, fixtureSpecSections]);

  const rows = useMemo(
    () => buildPlanListForest(filteredPlans, { specSectionsByPlan, panes }),
    [filteredPlans, specSectionsByPlan, panes],
  );

  return (
    <div
      data-testid="plan-list-view"
      className="relative flex h-full w-full flex-col overflow-hidden"
      style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}
    >
      {onClose && (
        <button
          type="button"
          data-testid="plan-list-close"
          onClick={onClose}
          className="absolute top-2 right-2 z-10 flex h-7 w-7 items-center justify-center rounded border cursor-pointer"
          style={{ background: "var(--ct-bg)", borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
          title="Close plan view"
        >
          <X size={14} />
        </button>
      )}
      {loading && (
        <div
          data-testid="plan-list-loading"
          className="flex flex-1 items-center justify-center text-sm"
          style={{ opacity: "var(--ct-text-secondary-opacity)" }}
        >
          Loading plans…
        </div>
      )}
      {!loading && error && (
        <div className="flex flex-1 items-center justify-center px-4 text-center text-sm" style={{ color: "var(--ct-error, #f87171)" }}>
          {error}
        </div>
      )}
      {!loading && !error && filteredPlans.length === 0 && (
        <div
          data-testid="plan-list-empty"
          className="flex flex-1 items-center justify-center px-6 text-center text-sm"
          style={{ opacity: "var(--ct-text-secondary-opacity)" }}
        >
          No active plans to list.
        </div>
      )}
      {!loading && !error && rows.length > 0 && (
        <div data-testid="plan-list-scroll" className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden py-1">
          {rows.map((row) => (
            <PlanListRowView key={row.id} row={row} />
          ))}
        </div>
      )}
    </div>
  );
}
