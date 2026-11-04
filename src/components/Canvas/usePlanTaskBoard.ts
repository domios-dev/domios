import { useCallback, useEffect, useRef, useState } from "react";
import { formatPlanFetchError } from "../../lib/fetchResilience";
import type { ApiConfig } from "../../lib/types";
import { fetchPlanTaskBoardPage } from "./plansApi";
import type { PlanTaskBoardPage, PlanTaskBoardStatus } from "./types";
import { useScopedPlanRead } from "./useScopedPlanRead";
import { planRevision } from "./planProjection";

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

export function usePlanTaskBoard(api: ApiConfig | undefined, planId: string | null, contextKey?: number): UsePlanTaskBoardResult {
  const query = useScopedPlanRead(api, planId, "board", contextKey, async (signal) => {
    const page = await fetchPlanTaskBoardPage(api!, planId!, { signal });
    if (page.plan_id !== planId) throw new Error("Task board response did not match the selected plan.");
    return page;
  });
  const [moreLoading, setMoreLoading] = useState<ReadonlySet<PlanTaskBoardStatus>>(new Set());
  const [moreErrors, setMoreErrors] = useState<ReadonlyMap<PlanTaskBoardStatus, string>>(new Map());
  const controllers = useRef(new Map<PlanTaskBoardStatus, AbortController>());
  const { data, setData, version } = query;

  useEffect(() => {
    const reads = controllers.current;
    for (const controller of reads.values()) controller.abort();
    reads.clear();
    setMoreLoading(new Set());
    setMoreErrors(new Map());
    return () => {
      for (const controller of reads.values()) controller.abort();
      reads.clear();
    };
  }, [api?.host, api?.port, api?.token, planId, contextKey, query.loading]);

  const loadNextPage = useCallback(async (status: PlanTaskBoardStatus) => {
    const current = data?.columns?.[status];
    if (!api || !planId || !current?.next_cursor || query.loading || controllers.current.has(status)) return;
    const requestVersion = version.current;
    const controller = new AbortController();
    controllers.current.set(status, controller);
    setMoreLoading((previous) => new Set(previous).add(status));
    setMoreErrors((previous) => {
      const next = new Map(previous);
      next.delete(status);
      return next;
    });
    try {
      const page = await fetchPlanTaskBoardPage(api, planId, { signal: controller.signal, status, cursor: current.next_cursor });
      if (controller.signal.aborted || version.current !== requestVersion) return;
      if (planRevision(page) !== planRevision(data)) { await query.refetch(); return; }
      if (page.plan_id !== planId || page.column?.status !== status) throw new Error("Task page response did not match the requested column.");
      const column = page.column;
      setData((previous) => {
        if (version.current !== requestVersion || !previous) return previous;
        const seen = new Set(current.items.map((item) => item.id));
        return { ...previous, columns: { ...previous.columns, [status]: {
          ...column, items: [...current.items, ...column.items.filter((item) => {
            if (seen.has(item.id)) return false;
            seen.add(item.id);
            return true;
          })],
        } } };
      });
    } catch (cause) {
      if (!controller.signal.aborted && version.current === requestVersion) {
        setMoreErrors((previous) => new Map(previous).set(status, formatPlanFetchError(cause)));
      }
    } finally {
      if (controllers.current.get(status) === controller) {
        controllers.current.delete(status);
        setMoreLoading((previous) => {
          const next = new Set(previous);
          next.delete(status);
          return next;
        });
      }
    }
  }, [api, planId, data, query.loading, query.refetch, setData, version]);

  return { ...query, reconnecting: false, loadNextPage,
    loadingStatus: (status) => moreLoading.has(status),
    errorForStatus: (status) => moreErrors.get(status) ?? null,
  };
}
