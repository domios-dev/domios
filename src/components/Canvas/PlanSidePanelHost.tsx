import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ApiConfig, Tab } from "../../lib/types";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import { planNavContextEqual, selectPlanNavContext } from "../../store/multiplexer/selectors";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { revealAssignedAgentTab } from "./planRevealAgentTab";
import { planSidePanelStore, usePlanSidePanelEntry, type PlanSidePanelTarget } from "./planSidePanelStore";
import { fetchPlan } from "./plansApi";
import { useFullPlans } from "./PlansProvider";
import { PlanSidePanel } from "./planView/PlanSidePanel";
import { usePlanTargetNavigation, type PlanTarget } from "./teamViewNavigation";
import type { Plan } from "./types";
import { usePresence, type PresenceMotion } from "./usePresence";

const PLAN_SIDE_PANEL_MOTION: PresenceMotion = { enter: "ct-plan-side-panel-in", exit: "ct-plan-side-panel-out" };

export interface PlanSidePanelLayerProps {
  plans: readonly Plan[];
  loading?: boolean;
  api?: ApiConfig;
  tabs?: Record<string, Tab>;
  onOpenTab?: (tabId: string) => void;
  onOpenPlanTarget: (target: PlanTarget) => void;
  onExited?: () => void;
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

export function PlanSidePanelLayer({ plans, loading = false, api, tabs, onOpenTab, onOpenPlanTarget, onExited }: PlanSidePanelLayerProps) {
  const entry = usePlanSidePanelEntry();
  const returnFocusTo = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    if (entry) returnFocusTo.current = null;
  }, [entry]);

  const finishExit = useCallback((panel: HTMLElement | null) => {
    const opener = returnFocusTo.current;
    returnFocusTo.current = null;
    const active = document.activeElement;
    const focusLeftBehind = !active || active === document.body || Boolean(panel?.contains(active));
    if (opener?.isConnected && focusLeftBehind) opener.focus();
    onExited?.();
  }, [onExited]);

  const presence = usePresence<PlanSidePanelTarget>(entry?.target ?? null, PLAN_SIDE_PANEL_MOTION, finishExit);
  const target = presence.item;
  const dismiss = useCallback(() => {
    const closed = planSidePanelStore.close();
    if (closed) returnFocusTo.current = closed.opener;
  }, []);

  const listed = target ? plans.find((plan) => plan.id === target.planId) ?? null : null;
  const unlisted = useUnlistedPlan(api, target && !listed ? target.planId : null);
  if (!target || !presence.state) return null;
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
        onClose={dismiss}
        state={presence.state}
        panelRef={presence.ref}
      />
    </div>
  );
}

const selectTabRecord = (state: ExtendedState) => state.tabs;

function OpenPlanSidePanel({ api, onExited }: { api: ApiConfig; onExited: () => void }) {
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
      onExited={onExited}
    />
  );
}

/** App-level host: one side panel over chat, terminal tabs and the team view alike, mounted until its exit finishes. */
export function PlanSidePanelHost({ api }: { api: ApiConfig }) {
  const open = usePlanSidePanelEntry() !== null;
  const [mounted, setMounted] = useState(open);
  if (open && !mounted) setMounted(true);
  const onExited = useCallback(() => setMounted(false), []);
  return mounted ? <OpenPlanSidePanel api={api} onExited={onExited} /> : null;
}
