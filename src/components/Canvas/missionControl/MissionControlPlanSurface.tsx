import { useCallback, useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { filterPlansForCanvas } from "../planTabMatch";
import { fixtureSpecSectionsByPlan } from "../perfFixture";
import { deletePlan, fetchSpecSections, parkPlan, setPlanMode } from "../plansApi";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import { PlanSwitcher } from "../planView/PlanSwitcher";
import { usePlanSwitcherActions } from "../planView/usePlanSwitcherActions";
import { enrichSwitcherOptions, selectActiveWork } from "../planView/activeWork";
import { orderPlansByActivity, selectDefaultPlanId } from "../planView/boardModel";
import { PlanDetailOverlay, type PlanDetailKind } from "../planView/PlanDetailOverlay";
import { PlanFetchErrorState, PlanFetchReconnectingBanner } from "../planView/PlanFetchErrorState";
import { planFetchShowsData } from "../planView/planViewFetchGate";
import type { Plan, Section, Task } from "../types";
import { useAllowanceWeeks } from "./allowanceWeek";
import { MissionControlBoard } from "./MissionControlBoard";
import { MissionControlControls } from "./MissionControlControls";
import { usePlanBoardState } from "./usePlanBoardState";
import type { NeedsYouDeepLinks } from "../../Notifications/NeedsYouNotificationRow";

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
  reconnecting = false,
  error = null,
  onRetry,
  planId,
  teamId,
  orchestratorTabId,
  tabs = {},
  workspaceId,
  workspaces,
  teams,
  onOpenTab,
  notifications,
  onSelectTab,
  api,
  headerActionsSlot = null,
}: PlanViewSurfaceProps) {
  const deepLinks = useMemo<NeedsYouDeepLinks | undefined>(
    () => (notifications && onSelectTab
      ? {
          notifications,
          tabs,
          workspaces: workspaces ?? [],
          scope: { teamId: teamId ?? null, workspaceId: workspaceId ?? null },
          onSelectTab,
        }
      : undefined),
    [notifications, onSelectTab, tabs, workspaces, teamId, workspaceId],
  );
  const { effectivePlans, switcherOptions, handleAction } = usePlanSwitcherActions(plans, api);

  const activeWork = useMemo(() => selectActiveWork(effectivePlans, tabs), [effectivePlans, tabs]);
  const switcherWithInflight = useMemo(
    () => enrichSwitcherOptions(switcherOptions, activeWork),
    [switcherOptions, activeWork],
  );

  const filteredPlans = useMemo(
    () => filterPlansForCanvas(effectivePlans, mode, { planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams }),
    [effectivePlans, mode, planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams],
  );
  const orderedPlans = useMemo(() => orderPlansByActivity(filteredPlans), [filteredPlans]);
  const defaultPlanId = useMemo(
    () => selectDefaultPlanId(orderedPlans, { planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams }),
    [orderedPlans, planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams],
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
      for (const plan of orderedPlans) {
        const task = plan.tasks.find((t) => t.id === taskId);
        if (task) {
          setDetail({ kind: "task", task, plan });
          return;
        }
      }
    },
    [orderedPlans],
  );

  const openPlanDetail = useCallback(
    (id?: string) => {
      const plan = id ? effectivePlans.find((p) => p.id === id) ?? selectedPlan : selectedPlan;
      if (!plan) return;
      const sections = plan.id === selectedPlan?.id ? selectedSpecSections : [];
      setDetail({ kind: "plan", plan, specSections: sections });
    },
    [effectivePlans, selectedPlan, selectedSpecSections],
  );

  const previousPlans = useMemo(() => {
    const teamOfPlan = selectedPlan?.team_id;
    if (!teamOfPlan) return [] as Plan[];
    return effectivePlans
      .filter((plan) => plan.status === "archived" && plan.team_id === teamOfPlan)
      .sort((a, b) => b.updated_at - a.updated_at);
  }, [effectivePlans, selectedPlan]);

  const openSpecDetail = useCallback(() => {
    if (!selectedPlan) return;
    setDetail({ kind: "spec", plan: selectedPlan, specSections: selectedSpecSections });
  }, [selectedPlan, selectedSpecSections]);

  const boardState = usePlanBoardState(api, selectedPlan?.id ?? null, selectedPlan?.updated_at ?? 0);
  const spendProviders = useMemo(
    () => boardState.state?.mission_control?.spend.providers.map((provider) => provider.provider) ?? [],
    [boardState.state],
  );
  const allowanceWeeks = useAllowanceWeeks(spendProviders);

  const approvePlan = useCallback((id: string) => {
    if (!api) return;
    void setPlanMode(api, id, "approved").catch(() => undefined);
  }, [api]);

  const pausePlan = useCallback((id: string) => {
    if (!api) return;
    void parkPlan(api, id, "Paused from Mission Control").catch(() => undefined);
  }, [api]);

  const clearEverything = useCallback((id: string) => {
    if (!api) return;
    void deletePlan(api, id).catch(() => undefined);
  }, [api]);

  const overlayKind: PlanDetailKind | null = detail?.kind ?? null;
  const showsData = planFetchShowsData(loading, error ?? null, plans.length);

  const showSwitcher = Boolean(selectedPlan && !teamId);
  const actions = (
    <>
      <PlanFetchReconnectingBanner
        reconnecting={reconnecting}
        itemCount={plans.length}
        testId="mission-control-reconnecting"
      />
      {showsData && selectedPlan && (
        <MissionControlControls
          scope="board"
          planId={selectedPlan.id}
          onApprovePlan={approvePlan}
          onPausePlan={pausePlan}
          onClearEverything={clearEverything}
        />
      )}
    </>
  );

  return (
    <div data-testid="mission-control-plan-surface" className="flex h-full w-full flex-col overflow-hidden">
      {headerActionsSlot && !showSwitcher ? (
        createPortal(actions, headerActionsSlot)
      ) : (
        <header
          data-testid="mission-control-surface-header"
          className="mission-control-surface-header flex shrink-0 items-center gap-3 border-b px-3 py-1.5"
          style={{ borderColor: "var(--ct-border)", background: "var(--ct-titlebar-bg)" }}
        >
          <div className="min-w-0 flex-1">
            {showSwitcher && selectedPlan && (
              <PlanSwitcher
                plans={switcherWithInflight}
                selectedId={selectedPlan.id}
                onSelect={selectPlan}
                onAction={handleAction}
              />
            )}
          </div>
          <div className="flex shrink-0 items-center gap-2">{actions}</div>
        </header>
      )}
      <div className="relative min-h-0 flex-1">
        <PlanFetchErrorState
          loading={loading}
          error={error}
          itemCount={plans.length}
          onRetry={onRetry}
          testId="mission-control"
          className="p-4"
        />
        {showsData && !selectedPlan && (
          <p data-testid="mission-control-empty" className="p-4 text-sm" style={{ color: "var(--ct-muted-fg)" }}>
            No active plans in this workspace.
          </p>
        )}
        {showsData && selectedPlan && (
          <MissionControlBoard
            plan={selectedPlan}
            state={boardState.state}
            loading={boardState.loading}
            error={boardState.error}
            allowanceWeeks={allowanceWeeks}
            plans={orderedPlans}
            previousPlans={previousPlans}
            onOpenTask={openTask}
            onOpenPlan={openPlanDetail}
            deepLinks={deepLinks}
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
          tabs={tabs}
          onOpenTab={onOpenTab}
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
