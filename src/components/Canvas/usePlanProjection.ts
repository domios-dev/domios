import type { ApiConfig } from "../../lib/types";
import { fetchPlanProjection } from "./plansApi";
import type { PlanProjectionView } from "./planProjection";
import { useScopedPlanRead } from "./useScopedPlanRead";

export function usePlanProjection(api: ApiConfig | undefined, planId: string | null, view: PlanProjectionView, contextKey?: number) {
  const query = useScopedPlanRead(api, planId, view, contextKey,
    (signal) => fetchPlanProjection(api!, planId!, view, signal), undefined,
    (change) => view !== "document" || change.reset || change.deleted);
  return { plan: query.data, loading: query.loading, error: query.error, retry: query.refetch };
}
