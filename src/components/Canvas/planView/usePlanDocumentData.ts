import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchSpecSections } from "../plansApi";
import { fixtureSpecSectionsByPlan } from "../perfFixture";
import type { Plan, Section } from "../types";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import {
  filterPlansForBoard,
  orderPlansByActivity,
  selectDefaultPlanId,
} from "./boardModel";
import { enrichSwitcherOptions, selectActiveWork } from "./activeWork";
import { usePlanSwitcherActions } from "./usePlanSwitcherActions";

const SELECTED_PLAN_STORAGE_KEY = "codeterm.board.selectedPlanId";

function readStoredPlanId(): string | null {
  try {
    return window.sessionStorage.getItem(SELECTED_PLAN_STORAGE_KEY);
  } catch {
    return null;
  }
}

export interface PlanDocumentData {
  selectedPlan: Plan | null;
  selectedSpecSections: Section[];
  specLoading: boolean;
  switcherOptions: ReturnType<typeof enrichSwitcherOptions>;
  selectPlan: (id: string) => void;
  handleAction: ReturnType<typeof usePlanSwitcherActions>["handleAction"];
  showsData: boolean;
}

export function usePlanDocumentData({
  plans,
  loading = false,
  error = null,
  mode,
  planId,
  teamId,
  orchestratorTabId,
  tabs = {},
  workspaceId,
  workspaces,
  teams,
  api,
}: PlanViewSurfaceProps): PlanDocumentData {
  const { effectivePlans, switcherOptions, handleAction } = usePlanSwitcherActions(plans, api);
  const activeWork = useMemo(
    () => selectActiveWork(effectivePlans, tabs),
    [effectivePlans, tabs],
  );
  const switcherWithInflight = useMemo(
    () => enrichSwitcherOptions(switcherOptions, activeWork),
    [switcherOptions, activeWork],
  );
  const [manualPlanId, setManualPlanId] = useState<string | null>(readStoredPlanId);
  const filteredPlans = useMemo(() => {
    const active = filterPlansForBoard(effectivePlans, mode, {
      planId,
      teamId,
      orchestratorTabId,
      tabs,
      workspaceId,
      workspaces,
      teams,
    });
    if (!manualPlanId || active.some((plan) => plan.id === manualPlanId)) return active;
    const archivedPlan = plans.find((plan) => plan.id === manualPlanId && plan.status === "archived");
    if (!archivedPlan || (mode === "focus" && planId && planId !== archivedPlan.id)) return active;
    return [...active, archivedPlan];
  }, [effectivePlans, manualPlanId, mode, planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams, plans]);
  const orderedPlans = useMemo(() => orderPlansByActivity(filteredPlans), [filteredPlans]);
  const defaultPlanId = useMemo(
    () => selectDefaultPlanId(orderedPlans, {
      planId,
      teamId,
      orchestratorTabId,
      tabs,
      workspaceId,
      workspaces,
      teams,
    }),
    [orderedPlans, planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams],
  );
  const selectedPlanId = useMemo(
    () => manualPlanId && orderedPlans.some((plan) => plan.id === manualPlanId)
      ? manualPlanId
      : defaultPlanId,
    [manualPlanId, orderedPlans, defaultPlanId],
  );
  const selectedPlan = useMemo(
    () => orderedPlans.find((plan) => plan.id === selectedPlanId) ?? null,
    [orderedPlans, selectedPlanId],
  );
  const selectPlan = useCallback((id: string) => {
    setManualPlanId(id);
    try {
      window.sessionStorage.setItem(SELECTED_PLAN_STORAGE_KEY, id);
    } catch {
      // Session persistence is best-effort; private-mode/quota failures must not block switching.
    }
  }, []);
  const fixtureSpecSections = useMemo(
    () => (selectedPlan ? fixtureSpecSectionsByPlan([selectedPlan]) : new Map<string, Section[]>()),
    [selectedPlan],
  );
  const [fetchedSpecSections, setFetchedSpecSections] = useState<Map<string, Section[]>>(new Map());
  const [specLoading, setSpecLoading] = useState(false);

  // Primitive keys only: mux rebuilds the plan object on every broadcast, and
  // this effect reaches the API.
  const specPlanId = selectedPlan?.id ?? "";
  const specSourceId = selectedPlan?.primary_spec_id ?? "";
  const specIsFixture = specPlanId ? fixtureSpecSections.has(specPlanId) : false;
  useEffect(() => {
    if (!api || !specSourceId || specIsFixture) {
      setSpecLoading(false);
      return;
    }
    let active = true;
    setSpecLoading(true);
    void fetchSpecSections(api, specSourceId)
      .catch(() => [] as Section[])
      .then((sections) => {
        if (!active) return;
        setFetchedSpecSections((previous) => new Map(previous).set(specPlanId, sections));
        setSpecLoading(false);
      });
    return () => {
      active = false;
    };
  }, [api?.host, api?.port, api?.token, specPlanId, specSourceId, specIsFixture]);

  const selectedSpecSections = useMemo(() => {
    if (!selectedPlan) return [];
    return fixtureSpecSections.get(selectedPlan.id) ?? fetchedSpecSections.get(selectedPlan.id) ?? [];
  }, [selectedPlan, fixtureSpecSections, fetchedSpecSections]);

  return {
    selectedPlan,
    selectedSpecSections,
    specLoading,
    switcherOptions: switcherWithInflight,
    selectPlan,
    handleAction,
    showsData: plans.length > 0 || (!loading && !error),
  };
}
