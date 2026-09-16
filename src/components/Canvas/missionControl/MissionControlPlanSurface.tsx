import { useCallback, useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { filterPlansForCanvas, isTeamActivePlan } from "../planTabMatch";
import {
  deletePlan,
  fetchPlanLifecyclePreview,
  parkPlan,
  resumePlan,
  setPlanMode,
} from "../plansApi";
import type { PlanLifecycleKind } from "../planLifecycle";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import { PlanSwitcher } from "../planView/PlanSwitcher";
import { usePlanSwitcherActions } from "../planView/usePlanSwitcherActions";
import { enrichSwitcherOptions, selectActiveWork } from "../planView/activeWork";
import { orderPlansByActivity, selectDefaultPlanId } from "../planView/boardModel";
import { PlanFetchErrorState, PlanFetchReconnectingBanner } from "../planView/PlanFetchErrorState";
import { planFetchShowsData } from "../planView/planViewFetchGate";
import type { Plan } from "../types";
import { MissionControlBoard } from "./MissionControlBoard";
import { MissionControlTeamBoard } from "./MissionControlTeamBoard";
import { MissionControlControls } from "./MissionControlControls";
import { usePlanBoardState } from "./usePlanBoardState";
import { useTeamMissionControl } from "./useTeamMissionControl";
import { useTeamSpend } from "./teamSpend";
import { planForDetail } from "./planDetailTarget";
import type { NeedsYouDeepLinks } from "../../Notifications/NeedsYouNotificationRow";

const SELECTED_PLAN_STORAGE_KEY = "codeterm.missionControl.selectedPlanId";

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
  notifications,
  onSelectTab,
  onClearNotifications,
  api,
  headerActionsSlot = null,
  onOpenView,
  onOpenPlanTarget,
}: PlanViewSurfaceProps) {
  const deepLinks = useMemo<NeedsYouDeepLinks | undefined>(
    () => (notifications && onSelectTab
      ? {
          notifications,
          tabs,
          workspaces: workspaces ?? [],
          scope: { teamId: teamId ?? null, workspaceId: workspaceId ?? null },
          onSelectTab,
          onClear: onClearNotifications,
        }
      : undefined),
    [notifications, onSelectTab, onClearNotifications, tabs, workspaces, teamId, workspaceId],
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

  const listedPlan = useMemo(
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

  const [boardEvents, setBoardEvents] = useState(0);
  const boardTeamId = teamId ?? listedPlan?.team_id ?? null;
  const plansRevision = useMemo(
    () => effectivePlans.reduce((latest, plan) => Math.max(latest, plan.updated_at), 0),
    [effectivePlans],
  );
  const teamRevision = `${plansRevision}:${boardEvents}`;
  const team = useTeamMissionControl(api, boardTeamId, teamRevision);
  // The daemon's team projection owns the active slot, so a draft absent from the plan feed still fills it.
  const selectedPlan = useMemo(() => {
    if (listedPlan || !teamId) return listedPlan;
    const projected = team?.activePlan ?? null;
    return projected && isTeamActivePlan(projected, teamId) ? projected : null;
  }, [listedPlan, teamId, team]);

  const previousPlans = useMemo(() => {
    const teamOfPlan = selectedPlan?.team_id;
    if (!teamOfPlan) return [] as Plan[];
    return effectivePlans
      .filter((plan) => plan.status === "archived" && plan.team_id === teamOfPlan)
      .sort((a, b) => b.updated_at - a.updated_at);
  }, [effectivePlans, selectedPlan]);

  const boardState = usePlanBoardState(api, selectedPlan?.id ?? null, selectedPlan?.updated_at ?? 0);
  const recordedEvents = boardState.state?.mission_control?.event_log.recorded_events ?? 0;
  useEffect(() => setBoardEvents(recordedEvents), [recordedEvents]);
  const teamSpend = useTeamSpend(api, boardTeamId, teamRevision);
  const boardPreviousPlans = team?.archivedPlans ?? previousPlans;
  const refPlans = useMemo(
    () => (team ? [...orderedPlans, ...team.archivedPlans] : orderedPlans),
    [orderedPlans, team],
  );

  const openTask = useCallback(
    (taskId: string) => {
      const plan = refPlans.find((candidate) => candidate.tasks.some((task) => task.id === taskId));
      if (!plan) return;
      onOpenPlanTarget?.({ planId: plan.id, taskId, teamId: plan.team_id });
    },
    [refPlans, onOpenPlanTarget],
  );

  const openPlanDetail = useCallback(
    (id?: string) => {
      const plan = planForDetail(id, selectedPlan, effectivePlans, team?.archivedPlans ?? []);
      if (!plan) return;
      onOpenPlanTarget?.({ planId: plan.id, teamId: plan.team_id });
    },
    [effectivePlans, selectedPlan, team, onOpenPlanTarget],
  );

  const approvePlan = useCallback((id: string) => {
    if (!api) return;
    void setPlanMode(api, id, "approved").catch(() => undefined);
  }, [api]);

  const previewPlanLifecycle = useCallback((id: string, kind: PlanLifecycleKind) => {
    if (!api) return Promise.reject(new Error("Plan API unavailable"));
    return fetchPlanLifecyclePreview(api, id, kind);
  }, [api]);

  const confirmPlanLifecycle = useCallback(async (id: string, kind: PlanLifecycleKind) => {
    if (!api) throw new Error("Plan API unavailable");
    if (kind === "pause") await parkPlan(api, id, "Paused from Mission Control");
    else await resumePlan(api, id);
  }, [api]);

  const clearEverything = useCallback((id: string) => {
    if (!api) return;
    void deletePlan(api, id).catch(() => undefined);
  }, [api]);

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
          planPaused={selectedPlan.mode === "paused"}
          onApprovePlan={approvePlan}
          onPreviewPlanLifecycle={previewPlanLifecycle}
          onConfirmPlanLifecycle={confirmPlanLifecycle}
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
        {showsData && !selectedPlan && team && (
          <MissionControlTeamBoard
            team={team}
            teamSpend={teamSpend}
            deepLinks={deepLinks}
            onOpenPlan={openPlanDetail}
          />
        )}
        {showsData && !selectedPlan && !team && (
          <div
            data-testid="mission-control-empty"
            className="flex h-full items-center justify-center p-8"
          >
            <p className="text-center text-[15px] text-[var(--ct-muted-fg)]">
              No active plans in this workspace.
            </p>
          </div>
        )}
        {showsData && selectedPlan && (
          <MissionControlBoard
            plan={selectedPlan}
            state={boardState.state}
            loading={boardState.loading}
            error={boardState.error}
            plans={refPlans}
            previousPlans={boardPreviousPlans}
            onOpenView={onOpenView}
            onOpenTask={openTask}
            onOpenPlan={openPlanDetail}
            deepLinks={deepLinks}
            eventPager={team?.events}
            teamSpend={teamSpend}
          />
        )}
      </div>
    </div>
  );
}
