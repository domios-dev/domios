import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { listen } from "../../lib/bridge";
import type { ApiConfig } from "../../lib/types";
import { fetchPlanIndex, type PlanIndexEntry } from "./plansApi";
import { usePlans, type UsePlansResult } from "./usePlans";

// ONE shared plan source for the whole app: a single lightweight index fetch +
// a single plan-changed subscription, consumed by every PlanButton. Full bodies
// are fetched once (batched) only while a canvas is open (ref-counted).
interface PlansContextValue {
  index: PlanIndexEntry[];
  indexLoading: boolean;
  indexError: string | null;
  full: UsePlansResult;
  acquireFull: () => void;
  releaseFull: () => void;
  acquireWorkspaceFull: (projectPath: string | null) => void;
  releaseWorkspaceFull: () => void;
}

const PlansContext = createContext<PlansContextValue | null>(null);

export function PlansProvider({ api, children }: { api: ApiConfig; children: ReactNode }) {
  const [index, setIndex] = useState<PlanIndexEntry[]>([]);
  const [indexLoading, setIndexLoading] = useState(true);
  const [indexError, setIndexError] = useState<string | null>(null);
  const apiRef = useRef(api);
  apiRef.current = api;
  const indexCtrl = useRef<AbortController | null>(null);

  const [fullRefCount, setFullRefCount] = useState(0);
  const [workspaceFullCount, setWorkspaceFullCount] = useState(0);
  const [workspaceProjectPath, setWorkspaceProjectPath] = useState<string | null>(null);
  const fullProject = workspaceFullCount > 0 ? workspaceProjectPath : null;
  const full = usePlans(api, { enabled: fullRefCount > 0, project: fullProject });

  const refetchIndex = useCallback(async () => {
    indexCtrl.current?.abort();
    const ctrl = new AbortController();
    indexCtrl.current = ctrl;
    setIndexError(null);
    try {
      const next = await fetchPlanIndex(apiRef.current, ctrl.signal);
      if (ctrl.signal.aborted) return;
      setIndex(next);
    } catch (e) {
      if (ctrl.signal.aborted || (e instanceof Error && e.name === "AbortError")) return;
      setIndexError(String(e));
    } finally {
      if (!ctrl.signal.aborted) setIndexLoading(false);
    }
  }, []);

  useEffect(() => {
    setIndexLoading(true);
    void refetchIndex();
    return () => indexCtrl.current?.abort();
  }, [api, refetchIndex]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    void listen<{ plan_id?: string }>("plan-changed", () => {
      if (active) void refetchIndex();
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
      indexError,
      full,
      acquireFull,
      releaseFull,
      acquireWorkspaceFull,
      releaseWorkspaceFull,
    }),
    [index, indexLoading, indexError, full, acquireFull, releaseFull, acquireWorkspaceFull, releaseWorkspaceFull],
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
  error: string | null;
}

// Lightweight consumer for PlanButton: visibility + pulse, no full bodies.
export function usePlanIndex(): PlanIndexResult {
  const { index, indexLoading, indexError } = usePlansContext();
  return { index, loading: indexLoading, error: indexError };
}

// Full-plan consumer for focus canvas: ref-counts so bodies load only while open.
export function useFullPlans(): UsePlansResult {
  const { full, acquireFull, releaseFull } = usePlansContext();
  useEffect(() => {
    acquireFull();
    return releaseFull;
  }, [acquireFull, releaseFull]);
  return full;
}

// Workspace or focus canvas: one ref-count path, project-scoped in workspace mode.
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
