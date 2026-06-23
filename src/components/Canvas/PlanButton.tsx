import { useEffect, useMemo } from "react";
import { Map } from "lucide-react";
import { listen } from "../../lib/bridge";
import type { ApiConfig, Pane } from "../../lib/types";
import { PANE_BAR_CONTROL_H } from "../Terminal/paneBarConstants";
import {
  canvasFocusStore,
  usePlanPulseVersion,
} from "./canvasFocusStore";
import {
  findPlansForPane,
  resolveOrchestratorPaneId,
} from "./planPaneMatch";
import { usePlans } from "./usePlans";

interface PlanButtonProps {
  paneId: string;
  api: ApiConfig;
  panes: Record<string, Pane>;
}

export function PlanButton({ paneId, api, panes }: PlanButtonProps) {
  const { plans } = usePlans(api);

  const relevantPlans = useMemo(
    () => findPlansForPane(plans, paneId, panes),
    [plans, paneId, panes],
  );
  const planIds = useMemo(() => relevantPlans.map((p) => p.id), [relevantPlans]);
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

  const open = () => {
    for (const id of planIds) canvasFocusStore.clearPulse(id);
    canvasFocusStore.openFocus({
      orchestratorPaneId,
      planId: relevantPlans.length === 1 ? relevantPlans[0].id : undefined,
    });
  };

  return (
    <button
      type="button"
      data-testid="plan-button"
      onClick={open}
      title="Open plan canvas"
      className={`flex shrink-0 items-center gap-1 rounded border px-2 text-[11px] font-medium cursor-pointer ${pulsing ? "ct-plan-button-pulse" : ""}`}
      style={{
        height: PANE_BAR_CONTROL_H,
        background: "var(--ct-bg)",
        borderColor: pulsing ? "var(--ct-accent)" : "var(--ct-border)",
        color: "var(--ct-fg)",
      }}
    >
      <Map size={13} />
      Plan
    </button>
  );
}
