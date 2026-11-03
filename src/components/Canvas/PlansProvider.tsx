import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { listen } from "../../lib/bridge";
import { useResilientQuery } from "../../lib/useResilientQuery";
import type { ApiConfig } from "../../lib/types";
import { fetchPlanIndex, type PlanIndexEntry } from "./plansApi";
import { usePlans, type UsePlansResult } from "./usePlans";
import { usePlanPeerContext } from "./usePlanPeerContext";
import { planBelongsToWorkspace, type PlanWorkspaceContext } from "./planTabMatch";

const EMPTY_INDEX: PlanIndexEntry[] = [];
const EMPTY_SUMMARIES: UsePlansResult = {
  plans: [],
  loading: false,
  reconnecting: false,
  error: null,
  refetch: async () => {},
};

interface PlansContextValue {
  api: ApiConfig;
  contextKey: number;
  index: PlanIndexEntry[];
  indexLoading: boolean;
  indexReconnecting: boolean;
  indexError: string | null;
  refetchIndex: () => Promise<void>;
  summaries: UsePlansResult;
  requestSummaries: () => () => void;
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
  const [summaryReaders, setSummaryReaders] = useState(0);
  const requestSummaries = useMemo(() => () => {
    setSummaryReaders((count) => count + 1);
    return () => setSummaryReaders((count) => count - 1);
  }, []);
  const summaries = usePlans(api, { enabled: queriesEnabled && summaryReaders > 0, contextKey: peerContext.generation });

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
      api,
      contextKey: peerContext.generation,
      index: queriesEnabled ? index : EMPTY_INDEX,
      indexLoading: queriesEnabled && indexLoading,
      indexReconnecting: queriesEnabled && indexReconnecting,
      indexError: queriesEnabled ? indexError : null,
      refetchIndex,
      summaries: queriesEnabled ? summaries : EMPTY_SUMMARIES,
      requestSummaries,
    }),
    [
      api,
      peerContext.generation,
      queriesEnabled,
      index,
      indexLoading,
      indexReconnecting,
      indexError,
      refetchIndex,
      summaries,
      requestSummaries,
    ],
  );

  return <PlansContext.Provider value={value}>{children}</PlansContext.Provider>;
}

function usePlansContext(): PlansContextValue {
  const ctx = useContext(PlansContext);
  if (!ctx) throw new Error("usePlansContext must be used within <PlansProvider>");
  return ctx;
}

export function usePlansApiOptional(): ApiConfig | undefined {
  return useContext(PlansContext)?.api;
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

export function usePlanSummaries(): UsePlansResult {
  const context = usePlansContext();
  useEffect(context.requestSummaries, [context.requestSummaries]);
  return context.summaries;
}

export function usePlanSummariesOptional(enabled = true): UsePlansResult | null {
  const context = useContext(PlansContext);
  const [requested, setRequested] = useState(false);
  const requestSummaries = context?.requestSummaries;
  useEffect(() => {
    if (!enabled || !requestSummaries) {
      setRequested(false);
      return;
    }
    setRequested(true);
    return requestSummaries();
  }, [enabled, requestSummaries]);
  if (!context || !enabled) return null;
  return {
    ...context.summaries,
    loading: !requested || context.summaries.loading,
  };
}

export function useSidebarPlansOptional(): UsePlansResult {
  return usePlanSummariesOptional() ?? EMPTY_SUMMARIES;
}

export function useCanvasPlans(
  mode: "workspace" | "focus",
  projectPath: string | null,
  scope?: PlanWorkspaceContext & { workspaceId: string | undefined },
): UsePlansResult {
  const full = usePlanSummaries();
  const plans = useMemo(() => {
    if (mode !== "workspace" || !projectPath) return full.plans;
    const expected = comparableProjectPath(projectPath);
    return full.plans.filter((plan) =>
      comparableProjectPath(plan.project_path) === expected ||
      Boolean(scope?.workspaceId && planBelongsToWorkspace(plan, scope.workspaceId, scope)),
    );
  }, [full.plans, mode, projectPath, scope?.workspaceId, scope?.workspaces, scope?.teams]);
  return useMemo(() => ({ ...full, plans }), [full, plans]);
}

function comparableProjectPath(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/^\/\/\?\//, "").replace(/\/+$/, "");
  return /^[a-z]:\//i.test(normalized) ? normalized.toLowerCase() : normalized;
}
