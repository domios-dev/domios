import { useCallback, useRef, useState } from "react";
import { publishNotice } from "../../../lib/notices";
import { PauseCircle, PlayCircle } from "lucide-react";
import type { ApiConfig } from "../../../lib/types";
import { parkPlan, resumePlan } from "../plansApi";
import type { Plan } from "../types";

export interface PlanModeControlsProps {
  plan: Pick<Plan, "id" | "mode" | "status">;
  api?: ApiConfig;
  /** A fixed pause reason; without one the owner is asked for it. */
  reason?: string;
  /** Explains what pausing does beside the button. */
  hint?: boolean;
}

export function isPlanParked(plan: Pick<Plan, "mode"> | null | undefined): boolean {
  return plan?.mode === "paused";
}

export const PLAN_PAUSE_HINT = "Pausing stops this plan's heartbeats and dispatch until you or the agent resume it.";

export function PlanModeControls({ plan, api, reason, hint = false }: PlanModeControlsProps) {
  const parked = isPlanParked(plan);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);

  const run = useCallback(async (action: () => Promise<void>, failure: string) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      await action();
    } catch (e) {
      publishNotice({ kind: "toast", severity: "Error", id: `plan-mode:${plan.id}`, title: failure, detail: e instanceof Error ? e.message : String(e) });
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [plan.id]);

  const onPark = useCallback(() => {
    if (!api) return;
    const trimmed = (reason ?? window.prompt("Reason for pausing this plan:") ?? "").trim();
    if (!trimmed) return;
    void run(() => parkPlan(api, plan.id, trimmed), "Plan pause failed");
  }, [api?.host, api?.port, api?.token, plan.id, reason, run]);

  const onResume = useCallback(() => {
    if (api) void run(() => resumePlan(api, plan.id), "Plan resume failed");
  }, [api?.host, api?.port, api?.token, plan.id, run]);

  return (
    <div className="plan-mode-controls" data-testid="plan-mode-controls">
      {parked ? (
        <button
          type="button"
          data-testid="plan-resume-button"
          className="plan-mode-button plan-mode-button-resume"
          onClick={onResume}
          disabled={!api || busy}
          title={api ? "Resume the paused plan" : "Connect to Domios to resume"}
        >
          <PlayCircle size={15} />
          <span>Resume</span>
        </button>
      ) : (
        <button
          type="button"
          data-testid="plan-park-button"
          className="plan-mode-button plan-mode-button-park"
          onClick={onPark}
          disabled={!api || busy || plan.status !== "active" || plan.mode === "complete"}
          title={api ? PLAN_PAUSE_HINT : "Connect to Domios to pause"}
        >
          <PauseCircle size={15} />
          <span>Pause</span>
        </button>
      )}
      {hint && <span className="plan-mode-hint">{parked ? "Paused: no heartbeats or dispatch until resumed." : PLAN_PAUSE_HINT}</span>}
    </div>
  );
}
