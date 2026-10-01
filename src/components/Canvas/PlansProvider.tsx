import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  type ReactNode,
} from "react";
import { listen } from "../../lib/bridge";
import { useResilientQuery } from "../../lib/useResilientQuery";
import type { ApiConfig } from "../../lib/types";
import { fetchPlanIndex, type PlanIndexEntry } from "./plansApi";
import { usePlans, type UsePlansResult } from "./usePlans";
import { usePlanPeerContext } from "./usePlanPeerContext";

const EMPTY_INDEX: PlanIndexEntry[] = [];
const EMPTY_FULL: UsePlansResult = {
  plans: [],
  loading: false,
  reconnecting: false,
  error: null,
  refetch: async () => {},
};

interface PlansContextValue {
  contextKey: number;
  index: PlanIndexEntry[];
  indexLoading: boolean;
  indexReconnecting: boolean;
  indexError: string | null;
  refetchIndex: () => Promise<void>;
  full: UsePlansResult;
}

const PlansContext = createContext<PlansContextValue | null>(null);

export function PlansProvider({
  api,
  enabled = true,
  children,
}: {
  api: ApiConfig;
  enabled?: boolean;
  children: ReactNode;
}) {
  const peerContext = usePlanPeerContext();
  const queriesEnabled = enabled && peerContext.ready;
  const full = usePlans(api, { enabled: queriesEnabled, contextKey: peerContext.generation });

  const {
    data: index,
    loading: indexLoading,
    reconnecting: indexReconnecting,
    error: indexError,
    refetch: refetchIndex,
  } = useResilientQuery((signal) => fetchPlanIndex(api, signal), EMPTY_INDEX, {
    enabled: queriesEnabled,
    deps: [api.host, api.port, api.token, peerContext.generation],
  });

  useEffect(() => {
    if (!queriesEnabled) return;
    let active = true;
    let unlistenPlanChanged: (() => void) | null = null;
    let unlistenPlansLoaded: (() => void) | null = null;
    void listen<{ plan_id?: string }>("plan-changed", () => {
      if (active) void refetchIndex();
    }).then((fn) => {
      if (!active) {
        fn();
        return;
      }
      unlistenPlanChanged = fn;
    });
    void listen("plans-loaded", () => {
      if (active) void refetchIndex();
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
  }, [queriesEnabled, refetchIndex]);

  const value = useMemo<PlansContextValue>(
    () => ({
      contextKey: peerContext.generation,
      index,
      indexLoading,
      indexReconnecting,
      indexError,
      refetchIndex,
      full,
    }),
    [
      peerContext.generation,
      index,
      indexLoading,
      indexReconnecting,
      indexError,
      refetchIndex,
      full,
    ],
  );

  return <PlansContext.Provider value={value}>{children}</PlansContext.Provider>;
}

function usePlansContext(): PlansContextValue {
  const ctx = useContext(PlansContext);
  if (!ctx) throw new Error("usePlansContext must be used within <PlansProvider>");
  return ctx;
}

export function usePlanDataContextKey(): number {
  return useContext(PlansContext)?.contextKey ?? 0;
}

export interface PlanIndexResult {
  index: PlanIndexEntry[];
  loading: boolean;
  reconnecting: boolean;
  error: string | null;
  refetch: () => Promise<void>;
}

export function usePlanIndex(): PlanIndexResult {
  const { index, indexLoading, indexReconnecting, indexError, refetchIndex } = usePlansContext();
  return {
    index,
    loading: indexLoading,
    reconnecting: indexReconnecting,
    error: indexError,
    refetch: refetchIndex,
  };
}

// Stable fallback: a fresh literal each render would churn every consumer's
// deps (e.g. resolvePlanRef) and re-render their subtree when no provider is up.
const EMPTY_PLAN_INDEX_RESULT: PlanIndexResult = {
  index: [], loading: false, reconnecting: false, error: null, refetch: async () => {},
};

export function usePlanIndexOptional(): PlanIndexResult {
  const ctx = useContext(PlansContext);
  if (!ctx) {
    return EMPTY_PLAN_INDEX_RESULT;
  }
  return {
    index: ctx.index,
    loading: ctx.indexLoading,
    reconnecting: ctx.indexReconnecting,
    error: ctx.indexError,
    refetch: ctx.refetchIndex,
  };
}

export function useFullPlans(): UsePlansResult {
  return usePlansContext().full;
}

export function useSidebarPlansOptional(): UsePlansResult {
  return useContext(PlansContext)?.full ?? EMPTY_FULL;
}

export function useCanvasPlans(mode: "workspace" | "focus", projectPath: string | null): UsePlansResult {
  const full = usePlansContext().full;
  const plans = useMemo(() => {
    if (mode !== "workspace" || !projectPath) return full.plans;
    const expected = comparableProjectPath(projectPath);
    return full.plans.filter((plan) => comparableProjectPath(plan.project_path) === expected);
  }, [full.plans, mode, projectPath]);
  return useMemo(() => ({ ...full, plans }), [full, plans]);
}

function comparableProjectPath(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/^\/\/\?\//, "").replace(/\/+$/, "");
  return /^[a-z]:\//i.test(normalized) ? normalized.toLowerCase() : normalized;
}
