import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import type { ApiConfig } from "../../lib/types";
import { fetchReconcile } from "./plansApi";
import type { DriftItem } from "./types";

interface PlanChangedPayload {
  plan_id?: string;
}

export function usePlanDrift(api: ApiConfig | undefined, planIds: string[]): Map<string, DriftItem[]> {
  const [driftByPlan, setDriftByPlan] = useState<Map<string, DriftItem[]>>(new Map());
  const ctrlRef = useRef<AbortController | null>(null);

  const refresh = useCallback(
    (targetPlanId?: string) => {
      if (!api) {
        setDriftByPlan(new Map());
        return;
      }
      if (targetPlanId && !planIds.includes(targetPlanId)) return;
      const ids = targetPlanId ? [targetPlanId] : planIds;
      if (!ids.length) {
        setDriftByPlan(new Map());
        return;
      }
      ctrlRef.current?.abort();
      const ctrl = new AbortController();
      ctrlRef.current = ctrl;
      void Promise.all(
        ids.map((id) =>
          fetchReconcile(api, id, ctrl.signal)
            .then((drift) => [id, drift] as const)
            .catch((e) => {
              if (ctrl.signal.aborted || (e instanceof Error && e.name === "AbortError")) return null;
              return [id, [] as DriftItem[]] as const;
            }),
        ),
      ).then((pairs) => {
        if (ctrl.signal.aborted) return;
        setDriftByPlan((prev) => {
          const next = targetPlanId ? new Map(prev) : new Map<string, DriftItem[]>();
          for (const pair of pairs) {
            if (!pair) continue;
            const [id, drift] = pair;
            if (drift.length) next.set(id, drift);
            else next.delete(id);
          }
          return next;
        });
      });
    },
    [api, planIds],
  );

  useEffect(() => {
    refresh();
    return () => ctrlRef.current?.abort();
  }, [refresh]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    void listen<PlanChangedPayload>("plan-changed", (ev) => {
      if (active) refresh(ev.payload?.plan_id);
    }).then((fn) => {
      if (!active) fn();
      else unlisten = fn;
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [refresh]);

  return driftByPlan;
}
