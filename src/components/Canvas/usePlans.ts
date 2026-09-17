import { useCallback, useEffect, useRef } from "react";
import { listen } from "../../lib/bridge";
import { useResilientQuery } from "../../lib/useResilientQuery";
import type { ApiConfig } from "../../lib/types";
import { fetchPlan, fetchPlans } from "./plansApi";
import type { Plan } from "./types";

const EMPTY_PLANS: Plan[] = [];

export interface UsePlansResult {
  plans: Plan[];
  loading: boolean;
  reconnecting: boolean;
  error: string | null;
  refetch: () => Promise<void>;
}

export interface UsePlansOptions {
  enabled?: boolean;
  project?: string | null;
}

interface PlanChangedPayload {
  plan_id: string;
}

export function usePlans(api: ApiConfig, options: UsePlansOptions = {}): UsePlansResult {
  const enabled = options.enabled ?? true;
  const project = options.project ?? null;
  const apiRef = useRef(api);
  apiRef.current = api;
  const projectRef = useRef(project);
  projectRef.current = project;
  const patchCtrls = useRef(new Map<string, AbortController>());

  const { data: plans, loading, reconnecting, error, refetch, setData } = useResilientQuery(
    (signal) =>
      fetchPlans(apiRef.current, signal, {
        project: projectRef.current,
      }),
    EMPTY_PLANS,
    { enabled, deps: [api.host, api.port, api.token, project] },
  );

  const patchPlan = useCallback(
    async (planId: string) => {
      if (!enabled) return;
      patchCtrls.current.get(planId)?.abort();
      const ctrl = new AbortController();
      patchCtrls.current.set(planId, ctrl);
      try {
        const plan = await fetchPlan(apiRef.current, planId, ctrl.signal);
        if (ctrl.signal.aborted) return;
        setData((prev) => {
          const idx = prev.findIndex((p) => p.id === planId);
          if (idx < 0) return [...prev, plan];
          const next = [...prev];
          next[idx] = plan;
          return next;
        });
      } catch (e) {
        if (ctrl.signal.aborted || (e instanceof Error && e.name === "AbortError")) return;
        await refetch();
      } finally {
        if (patchCtrls.current.get(planId) === ctrl) patchCtrls.current.delete(planId);
      }
    },
    [enabled, refetch, setData],
  );

  useEffect(() => {
    if (!enabled) {
      setData([]);
      return;
    }
    let active = true;
    let unlistenPlanChanged: (() => void) | null = null;
    let unlistenPlansLoaded: (() => void) | null = null;

    void listen<PlanChangedPayload>("plan-changed", (ev) => {
      if (!active) return;
      const planId = ev.payload?.plan_id;
      if (planId) void patchPlan(planId);
      else void refetch();
    }).then((fn) => {
      if (!active) {
        fn();
        return;
      }
      unlistenPlanChanged = fn;
    });
    void listen("plans-loaded", () => {
      if (active) void refetch();
    }).then((fn) => {
      if (!active) {
        fn();
        return;
      }
      unlistenPlansLoaded = fn;
    });

    return () => {
      active = false;
      unlistenPlanChanged?.();
      unlistenPlansLoaded?.();
    };
  }, [enabled, patchPlan, refetch, setData]);

  return { plans, loading, reconnecting, error, refetch };
}
