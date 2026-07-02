import { useCallback, useEffect, useMemo, useState } from "react";
import { X } from "lucide-react";
import { filterPlansForCanvas } from "../planPaneMatch";
import { fetchSpecSections } from "../plansApi";
import { fixtureSpecSectionsByPlan } from "../perfFixture";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import type { Plan, Section, Task } from "../types";
import { buildBoardPlanModel } from "./boardModel";
import { buildPlanListForest, type PlanListRow } from "./buildPlanListTree";
import { PlanContextHeader } from "./PlanContextHeader";
import { PlanDetailOverlay, type PlanDetailKind } from "./PlanDetailOverlay";
import { PlanListFilterBar } from "./PlanListFilterBar";
import { PlanListRowView } from "./PlanListRowView";
import {
  buildVisiblePlanListRows,
  groupPlanListRowsByPlan,
  resolveSectionExpanded,
  type PlanListStatusFilter,
} from "./planListModel";

type ListDetail =
  | { kind: "task"; task: Task; plan: Plan }
  | { kind: "plan"; plan: Plan }
  | { kind: "spec"; plan: Plan; specSections: Section[] };

function findTask(plan: Plan, taskId: string): Task | undefined {
  return plan.tasks.find((t) => t.id === taskId);
}

export function PlanListView({
  mode,
  plans,
  loading = false,
  error = null,
  planId,
  orchestratorPaneId,
  panes = {},
  onOpenPane,
  api,
  onClose,
}: PlanViewSurfaceProps) {
  const filteredPlans = useMemo(
    () => filterPlansForCanvas(plans, mode, { planId, orchestratorPaneId, panes }),
    [plans, mode, planId, orchestratorPaneId, panes],
  );
  const plansById = useMemo(() => new Map(filteredPlans.map((p) => [p.id, p])), [filteredPlans]);

  const fixtureSpecSections = useMemo(() => fixtureSpecSectionsByPlan(filteredPlans), [filteredPlans]);
  const [fetchedSpecSections, setFetchedSpecSections] = useState<Map<string, Section[]>>(new Map());
  const [statusFilter, setStatusFilter] = useState<PlanListStatusFilter>("all");
  const [textQuery, setTextQuery] = useState("");
  const [expansionOverrides, setExpansionOverrides] = useState<Map<string, boolean>>(new Map());
  const [detail, setDetail] = useState<ListDetail | null>(null);

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

  const planGroups = useMemo(() => groupPlanListRowsByPlan(rows), [rows]);

  const boardModelsByPlan = useMemo(() => {
    const map = new Map<string, ReturnType<typeof buildBoardPlanModel>>();
    for (const plan of filteredPlans) {
      map.set(plan.id, buildBoardPlanModel(plan, panes, specSectionsByPlan.get(plan.id)));
    }
    return map;
  }, [filteredPlans, panes, specSectionsByPlan]);

  const toggleSection = useCallback((sectionId: string, sectionRow?: PlanListRow) => {
    setExpansionOverrides((prev) => {
      const next = new Map(prev);
      const current =
        prev.get(sectionId) ??
        (sectionRow ? resolveSectionExpanded(sectionRow, prev) : true);
      next.set(sectionId, !current);
      return next;
    });
  }, []);

  const openRow = useCallback(
    (row: PlanListRow) => {
      const plan = plansById.get(row.planId);
      if (!plan) return;
      if (row.kind === "plan-root") {
        setDetail({ kind: "plan", plan });
        return;
      }
      if (row.kind === "spec-root") {
        setDetail({ kind: "spec", plan, specSections: specSectionsByPlan.get(plan.id) ?? [] });
        return;
      }
      if (row.taskId) {
        const task = findTask(plan, row.taskId);
        if (task) setDetail({ kind: "task", task, plan });
      }
    },
    [plansById, specSectionsByPlan],
  );

  const closeDetail = useCallback(() => setDetail(null), []);
  const overlayKind: PlanDetailKind | null = detail?.kind ?? null;

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
          className="absolute top-2 right-2 z-20 flex h-7 w-7 items-center justify-center rounded border cursor-pointer"
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
        <>
          <PlanListFilterBar
            statusFilter={statusFilter}
            textQuery={textQuery}
            onStatusFilter={setStatusFilter}
            onTextQuery={setTextQuery}
          />
          <div data-testid="plan-list-scroll" className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden py-1">
            {planGroups.map(({ planId: groupPlanId, rows: planRows }) => {
              const model = boardModelsByPlan.get(groupPlanId);
              const plan = plansById.get(groupPlanId);
              const visible = buildVisiblePlanListRows(planRows, {
                statusFilter,
                textQuery,
                expansionOverrides,
              });
              return (
                <section key={groupPlanId} data-plan-id={groupPlanId} className="plan-list-plan-group">
                  {model && plan && (
                    <div
                      className="sticky top-0 z-10"
                      style={{
                        background: "color-mix(in srgb, var(--ct-bg) 94%, transparent)",
                        backdropFilter: "blur(6px)",
                      }}
                    >
                      <PlanContextHeader
                        summary={model.planSummary}
                        spec={model.spec}
                        onOpenPlan={() => setDetail({ kind: "plan", plan })}
                        onOpenSpec={
                          model.spec
                            ? () =>
                                setDetail({
                                  kind: "spec",
                                  plan,
                                  specSections: specSectionsByPlan.get(plan.id) ?? [],
                                })
                            : undefined
                        }
                      />
                    </div>
                  )}
                  {visible.map((entry) => {
                    const sectionRow = entry.type === "row" ? entry.row : entry.sectionRow;
                    const sectionExpanded =
                      entry.type === "row" && sectionRow && (sectionRow.kind === "plan-section" || sectionRow.kind === "unsectioned" || sectionRow.kind === "spec-section")
                        ? resolveSectionExpanded(sectionRow, expansionOverrides)
                        : undefined;
                    return (
                      <PlanListRowView
                        key={entry.type === "row" ? entry.row.id : `rollup::${entry.sectionRow.id}`}
                        entry={entry}
                        sectionExpanded={sectionExpanded}
                        onToggleSection={(id) => {
                          const row = planRows.find((r) => r.id === id);
                          toggleSection(id, row);
                        }}
                        onOpenRow={openRow}
                      />
                    );
                  })}
                </section>
              );
            })}
          </div>
        </>
      )}
      {detail && overlayKind && (
        <PlanDetailOverlay
          kind={overlayKind}
          task={detail.kind === "task" ? detail.task : undefined}
          plan={detail.kind === "plan" ? detail.plan : detail.kind === "task" ? detail.plan : undefined}
          spec={detail.kind === "spec" ? detail.plan : undefined}
          specSections={detail.kind === "spec" ? detail.specSections : undefined}
          planId={detail.plan.id}
          panes={panes}
          onOpenPane={onOpenPane}
          onClose={closeDetail}
        />
      )}
    </div>
  );
}
