import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import type { ApiConfig } from "../../lib/types";
import { fetchPlan, fetchPlans } from "./plansApi";
import type { Plan } from "./types";

export interface UsePlansResult {
  plans: Plan[];
  loading: boolean;
  error: string | null;
  refetch: () => Promise<void>;
}

interface PlanChangedPayload {
  plan_id: string;
}

export function usePlans(api: ApiConfig): UsePlansResult {
  const [plans, setPlans] = useState<Plan[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const apiRef = useRef(api);
  apiRef.current = api;

  const refetch = useCallback(async () => {
    const ctrl = new AbortController();
    setLoading(true);
    setError(null);
    try {
      const next = await fetchPlans(apiRef.current, ctrl.signal);
      setPlans(next);
    } catch (e) {
      if (e instanceof Error && e.name === "AbortError") return;
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const patchPlan = useCallback(async (planId: string) => {
    const ctrl = new AbortController();
    try {
      const plan = await fetchPlan(apiRef.current, planId, ctrl.signal);
      setPlans((prev) => {
        const idx = prev.findIndex((p) => p.id === planId);
        if (idx < 0) return [...prev, plan];
        const next = [...prev];
        next[idx] = plan;
        return next;
      });
    } catch {
      await refetch();
    }
  }, [refetch]);

  useEffect(() => {
    let cancelled = false;
    const ctrl = new AbortController();
    setLoading(true);
    setError(null);

    fetchPlans(api, ctrl.signal)
      .then((next) => {
        if (cancelled) return;
        setPlans(next);
        setLoading(false);
      })
      .catch((e) => {
        if (cancelled || e?.name === "AbortError") return;
        setError(String(e));
        setLoading(false);
      });

    return () => {
      cancelled = true;
      ctrl.abort();
    };
  }, [api]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;

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
      unlisten = fn;
    });

    return () => {
      active = false;
      unlisten?.();
    };
  }, [patchPlan, refetch]);

  return { plans, loading, error, refetch };
}
