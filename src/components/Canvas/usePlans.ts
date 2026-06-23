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

export interface UsePlansOptions {
  // When false, no full bodies are fetched (PlanButton-only mode needs the index).
  enabled?: boolean;
  /** Scope list to a workspace project root (GET ?project=&full=1). */
  project?: string | null;
}

interface PlanChangedPayload {
  plan_id: string;
}

export function usePlans(api: ApiConfig, options: UsePlansOptions = {}): UsePlansResult {
  const enabled = options.enabled ?? true;
  const project = options.project ?? null;
  const [plans, setPlans] = useState<Plan[]>([]);
  const [loading, setLoading] = useState(enabled);
  const [error, setError] = useState<string | null>(null);
  const apiRef = useRef(api);
  apiRef.current = api;
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const projectRef = useRef(project);
  projectRef.current = project;
  // Last-write-wins: every fetch aborts the previous in-flight one.
  const listCtrl = useRef<AbortController | null>(null);
  const patchCtrls = useRef(new Map<string, AbortController>());

  const refetch = useCallback(async () => {
    if (!enabledRef.current) return;
    listCtrl.current?.abort();
    const ctrl = new AbortController();
    listCtrl.current = ctrl;
    setLoading(true);
    setError(null);
    try {
      const next = await fetchPlans(apiRef.current, ctrl.signal, {
        project: projectRef.current,
      });
      if (ctrl.signal.aborted) return;
      setPlans(next);
    } catch (e) {
      if (ctrl.signal.aborted || (e instanceof Error && e.name === "AbortError")) return;
      setError(String(e));
    } finally {
      if (!ctrl.signal.aborted) setLoading(false);
    }
  }, []);

  const patchPlan = useCallback(async (planId: string) => {
    if (!enabledRef.current) return;
    patchCtrls.current.get(planId)?.abort();
    const ctrl = new AbortController();
    patchCtrls.current.set(planId, ctrl);
    try {
      const plan = await fetchPlan(apiRef.current, planId, ctrl.signal);
      if (ctrl.signal.aborted) return;
      setPlans((prev) => {
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
  }, [refetch]);

  useEffect(() => {
    if (!enabled) {
      setPlans([]);
      setLoading(false);
      return;
    }
    const ctrl = new AbortController();
    listCtrl.current = ctrl;
    setLoading(true);
    setError(null);

    fetchPlans(api, ctrl.signal, { project })
      .then((next) => {
        if (ctrl.signal.aborted) return;
        setPlans(next);
        setLoading(false);
      })
      .catch((e) => {
        if (ctrl.signal.aborted || e?.name === "AbortError") return;
        setError(String(e));
        setLoading(false);
      });

    return () => ctrl.abort();
  }, [api, enabled, project]);

  useEffect(() => {
    if (!enabled) return;
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
  }, [enabled, patchPlan, refetch]);

  return { plans, loading, error, refetch };
}
