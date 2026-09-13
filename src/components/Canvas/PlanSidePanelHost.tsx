import { useCallback, useEffect, useState } from "react";
import type { ApiConfig, Tab } from "../../lib/types";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import { planNavContextEqual, selectPlanNavContext } from "../../store/multiplexer/selectors";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { revealAssignedAgentTab } from "./planRevealAgentTab";
import { planSidePanelStore, usePlanSidePanelEntry } from "./planSidePanelStore";
import { fetchPlan } from "./plansApi";
import { useFullPlans } from "./PlansProvider";
import { PlanSidePanel } from "./planView/PlanSidePanel";
import { usePlanTargetNavigation, type PlanTarget } from "./teamViewNavigation";
import type { Plan } from "./types";

export interface PlanSidePanelLayerProps {
  plans: readonly Plan[];
  loading?: boolean;
  api?: ApiConfig;
  tabs?: Record<string, Tab>;
  onOpenTab?: (tabId: string) => void;
  onOpenPlanTarget: (target: PlanTarget) => void;
}

/** A plan outside the loaded list, such as an archived one, is read on demand. */
function useUnlistedPlan(api: ApiConfig | undefined, planId: string | null) {
  const [result, setResult] = useState<{ planId: string; plan: Plan | null } | null>(null);
  useEffect(() => {
    if (!api || !planId) return;
    const controller = new AbortController();
    void fetchPlan(api, planId, controller.signal)
      .then((plan): Plan | null => plan, () => null)
      .then((plan) => {
        if (!controller.signal.aborted) setResult({ planId, plan });
      });
    return () => controller.abort();
  }, [api, planId]);
  const settled = result !== null && result.planId === planId;
  return { plan: settled ? result.plan : null, loading: Boolean(api && planId && !settled) };
}

function dismissPlanSidePanel() {
  const closed = planSidePanelStore.close();
  if (closed?.opener?.isConnected) closed.opener.focus();
}

export function PlanSidePanelLayer({ plans, loading = false, api, tabs, onOpenTab, onOpenPlanTarget }: PlanSidePanelLayerProps) {
  const entry = usePlanSidePanelEntry();
  const target = entry?.target ?? null;
  const listed = target ? plans.find((plan) => plan.id === target.planId) ?? null : null;
  const unlisted = useUnlistedPlan(api, target && !listed ? target.planId : null);
  if (!target) return null;
  return (
    <div className="plan-side-panel-layer" data-testid="plan-side-panel-layer">
      <PlanSidePanel
        target={target}
        plan={listed ?? unlisted.plan}
        loading={!listed && (loading || unlisted.loading)}
        api={api}
        tabs={tabs}
        onOpenTab={onOpenTab}
        onOpenPlanTarget={onOpenPlanTarget}
        onClose={dismissPlanSidePanel}
      />
    </div>
  );
}

const selectTabRecord = (state: ExtendedState) => state.tabs;

function OpenPlanSidePanel({ api }: { api: ApiConfig }) {
  const { plans, loading } = useFullPlans();
  const openPlanTarget = usePlanTargetNavigation();
  const tabs = useMuxSelector(selectTabRecord);
  const nav = useMuxSelector(selectPlanNavContext, planNavContextEqual);
  const { setActiveTab, setFocusedTab } = useMultiplexer();
  const onOpenTab = useCallback((tabId: string) => {
    planSidePanelStore.close();
    revealAssignedAgentTab({ workspaces: nav.workspaces, setActiveTab, setFocusedTab }, tabId);
  }, [nav.workspaces, setActiveTab, setFocusedTab]);
  return (
    <PlanSidePanelLayer
      plans={plans}
      loading={loading}
      api={api}
      tabs={tabs}
      onOpenTab={onOpenTab}
      onOpenPlanTarget={openPlanTarget}
    />
  );
}

/** App-level host: one side panel over chat, terminal tabs and the team view alike. */
export function PlanSidePanelHost({ api }: { api: ApiConfig }) {
  return usePlanSidePanelEntry() ? <OpenPlanSidePanel api={api} /> : null;
}
