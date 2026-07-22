import { useCallback, useRef, useState } from "react";
import { PauseCircle, PlayCircle } from "lucide-react";
import type { ApiConfig } from "../../../lib/types";
import { parkPlan, resumePlan } from "../plansApi";
import type { Plan } from "../types";

export interface PlanModeControlsProps {
  plan: Plan;
  api?: ApiConfig;
}

export function isPlanParked(plan: Plan | null | undefined): boolean {
  return plan?.mode === "paused";
}

export function PlanModeControls({ plan, api }: PlanModeControlsProps) {
  const parked = isPlanParked(plan);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const busyRef = useRef(false);

  const onPark = useCallback(async () => {
    if (!api || busyRef.current) return;
    const reason = window.prompt("Reason for parking this plan:");
    const trimmed = reason?.trim() ?? "";
    if (!trimmed) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      await parkPlan(api, plan.id, trimmed);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Plan park failed");
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [api, plan.id]);

  const onResume = useCallback(async () => {
    if (!api || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      await resumePlan(api, plan.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Plan resume failed");
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [api, plan.id]);

  return (
    <div className="plan-mode-controls" data-testid="plan-mode-controls">
      {parked ? (
        <button
          type="button"
          data-testid="plan-resume-button"
          className="plan-mode-button plan-mode-button-resume"
          onClick={onResume}
          disabled={!api || busy}
          title={api ? "Resume parked plan" : "Connect to CodeTerm to resume"}
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
          title={api ? "Park plan" : "Connect to CodeTerm to park"}
        >
          <PauseCircle size={15} />
          <span>Park</span>
        </button>
      )}
      {error && (
        <span data-testid="plan-mode-error" className="plan-mode-error" title={error}>
          Action failed
        </span>
      )}
    </div>
  );
}
