import {
  createContext,
  useCallback,
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

const EMPTY_INDEX: PlanIndexEntry[] = [];
const EMPTY_FULL: UsePlansResult = {
  plans: [],
  loading: false,
  reconnecting: false,
  error: null,
  refetch: async () => {},
};

interface PlansContextValue {
  index: PlanIndexEntry[];
  indexLoading: boolean;
  indexReconnecting: boolean;
  indexError: string | null;
  refetchIndex: () => Promise<void>;
  full: UsePlansResult;
  acquireFull: () => void;
  releaseFull: () => void;
  acquireWorkspaceFull: (projectPath: string | null) => void;
  releaseWorkspaceFull: () => void;
}

const PlansContext = createContext<PlansContextValue | null>(null);

export function PlansProvider({ api, children }: { api: ApiConfig; children: ReactNode }) {
  const [fullRefCount, setFullRefCount] = useState(0);
  const [workspaceFullCount, setWorkspaceFullCount] = useState(0);
  const [workspaceProjectPath, setWorkspaceProjectPath] = useState<string | null>(null);
  const fullProject = workspaceFullCount > 0 ? workspaceProjectPath : null;
  const full = usePlans(api, { enabled: fullRefCount > 0, project: fullProject });

  const {
    data: index,
    loading: indexLoading,
    reconnecting: indexReconnecting,
    error: indexError,
    refetch: refetchIndex,
  } = useResilientQuery((signal) => fetchPlanIndex(api, signal), EMPTY_INDEX, {
    deps: [api],
  });

  useEffect(() => {
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
  }, [refetchIndex]);

  const acquireFull = useCallback(() => setFullRefCount((n) => n + 1), []);
  const releaseFull = useCallback(() => setFullRefCount((n) => Math.max(0, n - 1)), []);
  const acquireWorkspaceFull = useCallback((projectPath: string | null) => {
    setWorkspaceProjectPath(projectPath);
    setWorkspaceFullCount((n) => n + 1);
    setFullRefCount((n) => n + 1);
  }, []);
  const releaseWorkspaceFull = useCallback(() => {
    setWorkspaceFullCount((n) => Math.max(0, n - 1));
    setFullRefCount((n) => Math.max(0, n - 1));
  }, []);

  const value = useMemo<PlansContextValue>(
    () => ({
      index,
      indexLoading,
      indexReconnecting,
      indexError,
      refetchIndex,
      full,
      acquireFull,
      releaseFull,
      acquireWorkspaceFull,
      releaseWorkspaceFull,
    }),
    [
      index,
      indexLoading,
      indexReconnecting,
      indexError,
      refetchIndex,
      full,
      acquireFull,
      releaseFull,
      acquireWorkspaceFull,
      releaseWorkspaceFull,
    ],
  );

  return <PlansContext.Provider value={value}>{children}</PlansContext.Provider>;
}

function usePlansContext(): PlansContextValue {
  const ctx = useContext(PlansContext);
  if (!ctx) throw new Error("usePlansContext must be used within <PlansProvider>");
  return ctx;
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
  const { full, acquireFull, releaseFull } = usePlansContext();
  useEffect(() => {
    acquireFull();
    return releaseFull;
  }, [acquireFull, releaseFull]);
  return full;
}

export function useSidebarPlansOptional(): UsePlansResult {
  const ctx = useContext(PlansContext);
  const acquireFull = ctx?.acquireFull;
  const releaseFull = ctx?.releaseFull;
  useEffect(() => {
    if (!acquireFull || !releaseFull) return;
    acquireFull();
    return releaseFull;
  }, [acquireFull, releaseFull]);
  return ctx?.full ?? EMPTY_FULL;
}

export function useCanvasPlans(mode: "workspace" | "focus", projectPath: string | null): UsePlansResult {
  const { full, acquireFull, releaseFull, acquireWorkspaceFull, releaseWorkspaceFull } =
    usePlansContext();
  useEffect(() => {
    if (mode === "workspace") {
      acquireWorkspaceFull(projectPath);
      return releaseWorkspaceFull;
    }
    acquireFull();
    return releaseFull;
  }, [
    mode,
    projectPath,
    acquireFull,
    releaseFull,
    acquireWorkspaceFull,
    releaseWorkspaceFull,
  ]);
  return full;
}
