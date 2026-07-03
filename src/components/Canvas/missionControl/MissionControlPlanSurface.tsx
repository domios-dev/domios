import { useCallback, useEffect, useMemo, useState } from "react";
import { filterPlansForCanvas } from "../planPaneMatch";
import { fixtureSpecSectionsByPlan } from "../perfFixture";
import { fetchSpecSections } from "../plansApi";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import { PlanSwitcher } from "../planView/PlanSwitcher";
import { usePlanSwitcherActions } from "../planView/usePlanSwitcherActions";
import { enrichSwitcherOptions, selectActiveWork } from "../planView/activeWork";
import { orderPlansByActivity, selectDefaultPlanId } from "../planView/boardModel";
import { PlanDetailOverlay, type PlanDetailKind } from "../planView/PlanDetailOverlay";
import type { Plan, Section, Task } from "../types";
import { MissionControlLiveView } from "./MissionControlLiveView";

const SELECTED_PLAN_STORAGE_KEY = "codeterm.missionControl.selectedPlanId";

type DetailState =
  | { kind: "task"; task: Task; plan: Plan }
  | { kind: "plan"; plan: Plan; specSections: Section[] }
  | { kind: "spec"; plan: Plan; specSections: Section[] };

function readStoredPlanId(): string | null {
  try {
    return window.sessionStorage.getItem(SELECTED_PLAN_STORAGE_KEY);
  } catch {
    return null;
  }
}

export function MissionControlPlanSurface({
  mode,
  plans,
  loading = false,
  error = null,
  planId,
  orchestratorPaneId,
  panes = {},
  onOpenPane,
  api,
}: PlanViewSurfaceProps) {
  const { effectivePlans, switcherOptions, handleAction } = usePlanSwitcherActions(plans, api);

  const activeWork = useMemo(() => selectActiveWork(effectivePlans, panes), [effectivePlans, panes]);
  const switcherWithInflight = useMemo(
    () => enrichSwitcherOptions(switcherOptions, activeWork),
    [switcherOptions, activeWork],
  );

  const filteredPlans = useMemo(
    () => filterPlansForCanvas(effectivePlans, mode, { planId, orchestratorPaneId, panes }),
    [effectivePlans, mode, planId, orchestratorPaneId, panes],
  );
  const orderedPlans = useMemo(() => orderPlansByActivity(filteredPlans), [filteredPlans]);
  const defaultPlanId = useMemo(
    () => selectDefaultPlanId(orderedPlans, { planId, orchestratorPaneId, panes }),
    [orderedPlans, planId, orchestratorPaneId, panes],
  );

  const [manualPlanId, setManualPlanId] = useState<string | null>(readStoredPlanId);
  const selectedPlanId = useMemo(() => {
    if (manualPlanId && orderedPlans.some((p) => p.id === manualPlanId)) return manualPlanId;
    return defaultPlanId;
  }, [manualPlanId, orderedPlans, defaultPlanId]);

  const selectedPlan = useMemo(
    () => orderedPlans.find((p) => p.id === selectedPlanId) ?? null,
    [orderedPlans, selectedPlanId],
  );

  const selectPlan = useCallback((id: string) => {
    setManualPlanId(id);
    try {
      window.sessionStorage.setItem(SELECTED_PLAN_STORAGE_KEY, id);
    } catch {
      // best-effort
    }
  }, []);

  const fixtureSpecSections = useMemo(
    () => (selectedPlan ? fixtureSpecSectionsByPlan([selectedPlan]) : new Map<string, Section[]>()),
    [selectedPlan],
  );
  const [fetchedSpecSections, setFetchedSpecSections] = useState<Map<string, Section[]>>(new Map());
  const [detail, setDetail] = useState<DetailState | null>(null);

  useEffect(() => {
    if (!api || !selectedPlan?.primary_spec_id) return;
    if (fixtureSpecSections.has(selectedPlan.id)) return;
    let active = true;
    const pid = selectedPlan.id;
    const specId = selectedPlan.primary_spec_id;
    void fetchSpecSections(api, specId)
      .then((sections) => (active ? [pid, sections] as const : null))
      .catch(() => (active ? [pid, [] as Section[]] as const : null))
      .then((pair) => {
        if (!active || !pair) return;
        setFetchedSpecSections((prev) => new Map(prev).set(pair[0], pair[1]));
      });
    return () => {
      active = false;
    };
  }, [api, selectedPlan, fixtureSpecSections]);

  const selectedSpecSections = useMemo(() => {
    if (!selectedPlan) return [] as Section[];
    return fixtureSpecSections.get(selectedPlan.id) ?? fetchedSpecSections.get(selectedPlan.id) ?? [];
  }, [selectedPlan, fixtureSpecSections, fetchedSpecSections]);

  const openTask = useCallback(
    (taskId: string) => {
      if (!selectedPlan) return;
      const task = selectedPlan.tasks.find((t) => t.id === taskId);
      if (task) setDetail({ kind: "task", task, plan: selectedPlan });
    },
    [selectedPlan],
  );

  const openPlanDetail = useCallback(() => {
    if (!selectedPlan) return;
    setDetail({ kind: "plan", plan: selectedPlan, specSections: selectedSpecSections });
  }, [selectedPlan, selectedSpecSections]);

  const openSpecDetail = useCallback(() => {
    if (!selectedPlan) return;
    setDetail({ kind: "spec", plan: selectedPlan, specSections: selectedSpecSections });
  }, [selectedPlan, selectedSpecSections]);

  const overlayKind: PlanDetailKind | null = detail?.kind ?? null;

  return (
    <div data-testid="mission-control-plan-surface" className="flex h-full w-full flex-col overflow-hidden">
      <header
        className="flex shrink-0 items-center gap-3 border-b px-3 py-1.5"
        style={{ borderColor: "var(--ct-border)", background: "var(--ct-titlebar-bg)" }}
      >
        {selectedPlan && (
          <PlanSwitcher
            plans={switcherWithInflight}
            selectedId={selectedPlan.id}
            onSelect={selectPlan}
            onAction={handleAction}
          />
        )}
      </header>
      <div className="relative min-h-0 flex-1">
        {loading && (
          <p data-testid="mission-control-loading" className="p-4 text-sm" style={{ color: "var(--ct-muted-fg)" }}>
            Loading plans…
          </p>
        )}
        {!loading && error && (
          <p data-testid="mission-control-error" className="p-4 text-sm" style={{ color: "var(--ct-error, #f87171)" }}>
            {error}
          </p>
        )}
        {!loading && !error && !selectedPlan && (
          <p data-testid="mission-control-empty" className="p-4 text-sm" style={{ color: "var(--ct-muted-fg)" }}>
            No active plans in this workspace.
          </p>
        )}
        {!loading && !error && selectedPlan && (
          <MissionControlLiveView
            plan={selectedPlan}
            panes={panes}
            onTaskClick={openTask}
            onOpenPlan={openPlanDetail}
          />
        )}
      </div>
      {detail && overlayKind && (
        <PlanDetailOverlay
          kind={overlayKind}
          task={detail.kind === "task" ? detail.task : undefined}
          plan={detail.kind === "plan" ? detail.plan : detail.kind === "task" ? detail.plan : undefined}
          spec={detail.kind === "spec" ? detail.plan : undefined}
          specSections={detail.kind === "spec" || detail.kind === "plan" ? detail.specSections : undefined}
          planId={detail.plan.id}
          api={api}
          panes={panes}
          onOpenPane={onOpenPane}
          onOpenTask={openTask}
          onClose={() => setDetail(null)}
          onSectionClick={(args) => {
            if (args.variant === "spec") openSpecDetail();
            else openPlanDetail();
          }}
        />
      )}
    </div>
  );
}
