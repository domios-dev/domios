import { useEffect, useMemo } from "react";
import { Map } from "lucide-react";
import { listen } from "../../lib/bridge";
import type { Tab } from "../../lib/types";
import { TAB_BAR_CONTROL_H } from "../Terminal/tabBarConstants";
import { ActivityDots } from "../shared/ActivityDots";
import {
  canvasFocusStore,
  usePlanPulseVersion,
} from "./canvasFocusStore";
import {
  findPlanIndexForTab,
  resolveOrchestratorTabId,
} from "./planTabMatch";
import { usePlanIndex } from "./PlansProvider";

interface PlanButtonProps {
  tabId: string;
  tabs: Record<string, Tab>;
}

export function PlanButton({ tabId, tabs }: PlanButtonProps) {
  const { index } = usePlanIndex();

  const relevantPlans = useMemo(
    () => findPlanIndexForTab(index, tabId, tabs),
    [index, tabId, tabs],
  );
  const planIds = useMemo(() => relevantPlans.map((p) => p.plan_id), [relevantPlans]);
  usePlanPulseVersion(planIds);

  useEffect(() => {
    if (planIds.length === 0) return;
    let active = true;
    let unlisten: (() => void) | null = null;
    const ids = new Set(planIds);
    void listen<{ plan_id?: string }>("plan-changed", (ev) => {
      const id = ev.payload?.plan_id;
      if (id && ids.has(id) && !canvasFocusStore.isOpen()) {
        canvasFocusStore.markPulse(id);
      }
    }).then((fn) => {
      if (!active) {
        fn();
        return;
      }
      unlisten = fn;
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [planIds]);

  if (relevantPlans.length === 0) return null;

  const pulsing = canvasFocusStore.anyPulsing(planIds);
  const orchestratorTabId = resolveOrchestratorTabId(tabId, tabs);

  const open = () => {
    for (const id of planIds) canvasFocusStore.clearPulse(id);
    canvasFocusStore.openFocus({
      orchestratorTabId,
      planId: relevantPlans.length === 1 ? relevantPlans[0].plan_id : undefined,
      focusTabId: tabId,
    });
  };

  return (
    <button
      type="button"
      data-testid="plan-button"
      onClick={open}
      title="Open plan canvas"
      className="flex shrink-0 items-center gap-1 rounded border px-2 text-[11px] font-medium cursor-pointer"
      style={{
        height: TAB_BAR_CONTROL_H,
        background: "var(--ct-bg)",
        borderColor: pulsing ? "var(--ct-accent)" : "var(--ct-border)",
        color: "var(--ct-fg)",
      }}
    >
      <Map size={13} />
      Plan
      {pulsing && <ActivityDots data-testid="plan-button-activity" size="small" tone="muted" active />}
      {pulsing && <span className="sr-only" role="status" aria-live="polite">Plan updated</span>}
    </button>
  );
}
