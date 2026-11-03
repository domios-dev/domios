import { useCallback, useEffect, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import { formatPlanFetchError } from "../../lib/fetchResilience";
import { fetchPlanListWindow, PlanReadError } from "./plansApi";
import { planRevision } from "./planProjection";
import { useScopedPlanRead } from "./useScopedPlanRead";

export function usePlanListWindow(api: ApiConfig | undefined, planId: string | null, status: string, search: string) {
  const query = useScopedPlanRead(api, planId, JSON.stringify(["list", status, search]), undefined,
    (signal) => fetchPlanListWindow(api!, planId!, status, search, signal));
  const { data, setData, version, refetch } = query;
  const controller = useRef<AbortController | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [moreError, setMoreError] = useState<string | null>(null);
  useEffect(() => {
    controller.current?.abort(); controller.current = null; setLoadingMore(false); setMoreError(null);
    return () => { controller.current?.abort(); controller.current = null; };
  }, [api?.host, api?.port, api?.token, planId, status, search, query.loading]);
  const loadMore = useCallback(async () => {
    const cursor = data?.task_window.next_cursor;
    if (!api || !planId || !cursor || query.loading || controller.current) return;
    const request = new AbortController(); controller.current = request;
    const generation = version.current;
    setLoadingMore(true); setMoreError(null);
    try {
      const page = await fetchPlanListWindow(api, planId, status, search, request.signal, cursor);
      if (request.signal.aborted || version.current !== generation) return;
      if (planRevision(page) !== planRevision(data)) { await refetch(); return; }
      setData((previous) => {
        if (!previous || version.current !== generation || previous.task_window.next_cursor !== cursor) return previous;
        const seen = new Set(previous.tasks.map((task) => task.id));
        return { ...page, tasks: [...previous.tasks, ...page.tasks.filter((task) => { if (seen.has(task.id)) return false; seen.add(task.id); return true; })] };
      });
    } catch (cause) {
      if (!request.signal.aborted && version.current === generation) {
        if (cause instanceof PlanReadError && [400,401,403,404,410].includes(cause.status)) await refetch();
        else setMoreError(formatPlanFetchError(cause));
      }
    } finally {
      if (controller.current === request) { controller.current = null; setLoadingMore(false); }
    }
  }, [api?.host, api?.port, api?.token, planId, status, search, data, query.loading, setData, version, refetch]);
  return { plan: data, loading: query.loading, error: query.error, retry: refetch, loadMore, loadingMore, moreError };
}
