import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { useResilientQuery } from "../../lib/useResilientQuery";
import { formatPlanFetchError } from "../../lib/fetchResilience";
import type { ApiConfig } from "../../lib/types";
import { fetchPlan, fetchPlanTaskBoardPage, fetchPlanTaskDetail, fetchPlans } from "./plansApi";
import type { Plan, PlanTaskBoardPage, PlanTaskBoardStatus, PlanTaskDetail } from "./types";

const EMPTY_PLANS: Plan[] = [];

export interface UsePlansResult {
  plans: Plan[];
  loading: boolean;
  reconnecting: boolean;
  error: string | null;
  refetch: () => Promise<void>;
}

export interface UsePlansOptions {
  chat?: boolean;
  enabled?: boolean;
  project?: string | null;
  contextKey?: number;
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
        chat: options.chat,
      }),
    EMPTY_PLANS,
    { enabled, deps: [api.host, api.port, api.token, project, options.contextKey, options.chat] },
  );

  const patchPlan = useCallback(
    async (planId: string) => {
      if (!enabled) return;
      patchCtrls.current.get(planId)?.abort();
      const ctrl = new AbortController();
      patchCtrls.current.set(planId, ctrl);
      try {
        if (options.chat) { await refetch(); return; }
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
    [enabled, refetch, setData, options.chat],
  );

  useEffect(() => {
    if (!enabled) {
      setData([]);
      return;
    }
    let active = true;
    const controllers = patchCtrls.current;
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
      for (const controller of controllers.values()) controller.abort();
      controllers.clear();
      unlistenPlanChanged?.();
      unlistenPlansLoaded?.();
    };
  }, [enabled, patchPlan, refetch, setData, options.contextKey]);

  return useMemo(() => ({ plans, loading, reconnecting, error, refetch }), [plans, loading, reconnecting, error, refetch]);
}

export interface UsePlanTaskBoardResult {
  data: PlanTaskBoardPage | null;
  loading: boolean;
  reconnecting: boolean;
  error: string | null;
  refetch: () => Promise<void>;
  loadNextPage: (status: PlanTaskBoardStatus) => Promise<void>;
  loadingStatus: (status: PlanTaskBoardStatus) => boolean;
  errorForStatus: (status: PlanTaskBoardStatus) => string | null;
}

const EMPTY_PAGE: PlanTaskBoardPage | null = null;

export function usePlanTaskBoard(
  api: ApiConfig | undefined,
  planId: string | null,
  contextKey?: number,
): UsePlanTaskBoardResult {
  const [moreLoading, setMoreLoading] = useState<ReadonlySet<PlanTaskBoardStatus>>(new Set());
  const [moreErrors, setMoreErrors] = useState<ReadonlyMap<PlanTaskBoardStatus, string>>(new Map());
  const [initial, setInitial] = useState<{
    planId: string | null;
    data: PlanTaskBoardPage | null;
    loading: boolean;
    error: string | null;
  }>({ planId: null, data: EMPTY_PAGE, loading: false, error: null });
  const [retryKey, setRetryKey] = useState(0);
  const controllers = useRef(new Map<PlanTaskBoardStatus, AbortController>());
  const planIdRef = useRef(planId);
  planIdRef.current = planId;

  useEffect(() => {
    if (!api || !planId) {
      setInitial({ planId: null, data: EMPTY_PAGE, loading: false, error: null });
      return;
    }
    const controller = new AbortController();
    setInitial({ planId, data: null, loading: true, error: null });
    void fetchPlanTaskBoardPage(api, planId, { signal: controller.signal })
      .then((data) => {
        if (!controller.signal.aborted) setInitial({ planId, data, loading: false, error: null });
      })
      .catch((cause) => {
        if (controller.signal.aborted || (cause instanceof Error && cause.name === "AbortError")) return;
        setInitial({ planId, data: null, loading: false, error: formatPlanFetchError(cause) });
      });
    return () => controller.abort();
  }, [api?.host, api?.port, api?.token, planId, contextKey, retryKey]);

  const refetch = useCallback(async () => {
    setRetryKey((key) => key + 1);
  }, []);

  useEffect(() => {
    for (const controller of controllers.current.values()) controller.abort();
    controllers.current.clear();
    setMoreLoading(new Set());
    setMoreErrors(new Map());
    return () => {
      for (const controller of controllers.current.values()) controller.abort();
      controllers.current.clear();
    };
  }, [api?.host, api?.port, api?.token, planId, contextKey, retryKey]);

  const loadNextPage = useCallback(async (status: PlanTaskBoardStatus) => {
    if (!api || !planId) return;
    const currentPage = initial.planId === planId ? initial.data : null;
    const current = currentPage?.columns?.[status];
    if (current && !current.next_cursor) return;
    controllers.current.get(status)?.abort();
    const controller = new AbortController();
    controllers.current.set(status, controller);
    setMoreLoading((prev) => new Set(prev).add(status));
    setMoreErrors((prev) => {
      const next = new Map(prev);
      next.delete(status);
      return next;
    });
    try {
      const result = await fetchPlanTaskBoardPage(api, planId, {
        signal: controller.signal,
        status,
        cursor: current?.next_cursor,
      });
      if (controller.signal.aborted || planIdRef.current !== planId || !result.column) return;
      setInitial((previous) => {
        if (!previous.data || previous.planId !== planId) return previous;
        const oldColumn = previous.data.columns?.[status];
        const seen = new Set(oldColumn?.items.map((item) => item.id) ?? []);
        const items = [...(oldColumn?.items ?? []), ...result.column.items.filter((item) => !seen.has(item.id))];
        return {
          ...previous,
          data: {
            ...previous.data,
            columns: {
              ...previous.data.columns,
              [status]: { ...result.column, items },
            },
          },
        };
      });
    } catch (error) {
      if (controller.signal.aborted || (error instanceof Error && error.name === "AbortError")) return;
      setMoreErrors((prev) => new Map(prev).set(
        status,
        error instanceof Error ? error.message : "Could not load more tasks.",
      ));
    } finally {
      if (controllers.current.get(status) === controller) {
        controllers.current.delete(status);
        setMoreLoading((prev) => {
          const next = new Set(prev);
          next.delete(status);
          return next;
        });
      }
    }
  }, [api, planId, initial.planId, initial.data]);

  useEffect(() => {
    if (!api || !planId) return;
    let active = true;
    let unlisten: (() => void) | null = null;
    void listen<{ plan_id?: string }>("plan-changed", (event) => {
      if (active && event.payload?.plan_id === planId) {
        for (const controller of controllers.current.values()) controller.abort();
        controllers.current.clear();
        setMoreLoading(new Set());
        void refetch();
      }
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [api, planId, refetch]);

  return {
    data: initial.planId === planId ? initial.data : null,
    loading: Boolean(api && planId) && (initial.planId !== planId || initial.loading),
    reconnecting: false,
    error: initial.planId === planId ? initial.error : null,
    refetch,
    loadNextPage,
    loadingStatus: (status) => moreLoading.has(status),
    errorForStatus: (status) => moreErrors.get(status) ?? null,
  };
}

export interface UsePlanTaskDetailResult {
  detail: PlanTaskDetail | null;
  loading: boolean;
  error: string | null;
  retry: () => Promise<void>;
}

interface PlanTaskDetailState {
  key: string | null;
  detail: PlanTaskDetail | null;
  loading: boolean;
  error: string | null;
}

export function usePlanTaskDetail(
  api: ApiConfig | undefined,
  planId: string | null,
  taskId: string | null,
  contextKey?: number,
): UsePlanTaskDetailResult {
  const key = planId && taskId ? `${planId}\u0000${taskId}` : null;
  const [state, setState] = useState<PlanTaskDetailState>({ key: null, detail: null, loading: false, error: null });
  const [retryKey, setRetryKey] = useState(0);

  useEffect(() => {
    if (!api || !planId || !taskId || !key) {
      setState({ key: null, detail: null, loading: false, error: null });
      return;
    }
    const controller = new AbortController();
    setState({ key, detail: null, loading: true, error: null });
    void fetchPlanTaskDetail(api, planId, taskId, controller.signal)
      .then((detail) => {
        if (controller.signal.aborted) return;
        if (detail.plan_id !== planId || detail.task_id !== taskId || detail.task.id !== taskId || detail.context.plan_id !== planId) {
          throw new Error("Task detail response did not match the selected task.");
        }
        setState({ key, detail, loading: false, error: null });
      })
      .catch((cause) => {
        if (controller.signal.aborted || (cause instanceof Error && cause.name === "AbortError")) return;
        setState({ key, detail: null, loading: false, error: formatPlanFetchError(cause) });
      });
    return () => controller.abort();
  }, [api?.host, api?.port, api?.token, planId, taskId, contextKey, retryKey, key]);

  const retry = useCallback(async () => {
    setRetryKey((previous) => previous + 1);
  }, []);
  const current = state.key === key;
  return useMemo(() => ({
    detail: current ? state.detail : null,
    loading: Boolean(api && key && (!current || state.loading)),
    error: current ? state.error : null,
    retry,
  }), [api, key, current, state.detail, state.loading, state.error, retry]);
}

export interface UsePlanByIdResult {
  plan: Plan | null;
  loading: boolean;
  error: string | null;
  retry: () => void;
}

export function usePlanById(
  api: ApiConfig | undefined,
  planId: string | null,
  contextKey?: number,
): UsePlanByIdResult {
  const [result, setResult] = useState<{ planId: string; plan: Plan | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [retryKey, setRetryKey] = useState(0);

  useEffect(() => {
    if (!api || !planId) {
      setResult(null);
      setError(null);
      return;
    }
    const controller = new AbortController();
    setResult(null);
    setError(null);
    void fetchPlan(api, planId, controller.signal)
      .then((plan) => {
        if (!controller.signal.aborted) setResult({ planId, plan });
      })
      .catch((cause) => {
        if (controller.signal.aborted || (cause instanceof Error && cause.name === "AbortError")) return;
        setResult({ planId, plan: null });
        setError(formatPlanFetchError(cause));
      });
    return () => controller.abort();
  }, [api?.host, api?.port, api?.token, planId, contextKey, retryKey]);

  return {
    plan: result?.planId === planId ? result.plan : null,
    loading: Boolean(api && planId && (result?.planId !== planId) && !error),
    error: result?.planId === planId ? error : null,
    retry: () => setRetryKey((key) => key + 1),
  };
}
