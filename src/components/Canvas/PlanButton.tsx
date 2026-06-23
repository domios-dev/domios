import { useEffect, useMemo } from "react";
import { Map } from "lucide-react";
import { listen } from "../../lib/bridge";
import type { Pane } from "../../lib/types";
import { PANE_BAR_CONTROL_H } from "../Terminal/paneBarConstants";
import {
  canvasFocusStore,
  usePlanPulseVersion,
} from "./canvasFocusStore";
import {
  findPlanIndexForPane,
  resolveOrchestratorPaneId,
} from "./planPaneMatch";
import { usePlanIndex } from "./PlansProvider";

interface PlanButtonProps {
  paneId: string;
  panes: Record<string, Pane>;
}

export function PlanButton({ paneId, panes }: PlanButtonProps) {
  const { index } = usePlanIndex();

  const relevantPlans = useMemo(
    () => findPlanIndexForPane(index, paneId, panes),
    [index, paneId, panes],
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
  const orchestratorPaneId = resolveOrchestratorPaneId(paneId, panes);

  // Role-driven label (§7): orchestrator owns a plan → "Plan"; a worker bound to
  // a task only → "Tasks". Owner-match (self or pane chain) decides.
  const ownsPlan = relevantPlans.some(
    (p) => p.owner.pane_id === paneId || p.owner.pane_id === orchestratorPaneId,
  );
  const label = ownsPlan ? "Plan" : "Tasks";

  const open = () => {
    for (const id of planIds) canvasFocusStore.clearPulse(id);
    canvasFocusStore.openFocus({
      orchestratorPaneId: ownsPlan ? orchestratorPaneId : paneId,
      planId: relevantPlans.length === 1 ? relevantPlans[0].plan_id : undefined,
    });
  };

  return (
    <button
      type="button"
      data-testid="plan-button"
      data-role-label={label}
      onClick={open}
      title={ownsPlan ? "Open plan canvas" : "Open your tasks on the canvas"}
      className={`flex shrink-0 items-center gap-1 rounded border px-2 text-[11px] font-medium cursor-pointer ${pulsing ? "ct-plan-button-pulse" : ""}`}
      style={{
        height: PANE_BAR_CONTROL_H,
        background: "var(--ct-bg)",
        borderColor: pulsing ? "var(--ct-accent)" : "var(--ct-border)",
        color: "var(--ct-fg)",
      }}
    >
      <Map size={13} />
      {label}
    </button>
  );
}
