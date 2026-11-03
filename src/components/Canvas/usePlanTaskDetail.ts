import type { ApiConfig } from "../../lib/types";
import { fetchPlanTaskDetail } from "./plansApi";
import { taskDisplayKey } from "./taskDisplayKey";
import type { PlanTaskDetail } from "./types";
import { useScopedPlanRead } from "./useScopedPlanRead";

export interface UsePlanTaskDetailResult {
  detail: PlanTaskDetail | null;
  loading: boolean;
  error: string | null;
  retry: () => Promise<void>;
}

export function taskDetailMatchesReference(detail: PlanTaskDetail, planId: string, reference: string): boolean {
  const requested = reference.trim().toLowerCase();
  const matches = detail.task.id === reference || [detail.task.handle, taskDisplayKey(detail.task)]
    .some((candidate) => candidate?.toLowerCase() === requested);
  return detail.plan_id === planId && detail.context.plan_id === planId && detail.task_id === detail.task.id && matches;
}

export function usePlanTaskDetail(api: ApiConfig | undefined, planId: string | null, taskId: string | null, contextKey?: number): UsePlanTaskDetailResult {
  const query = useScopedPlanRead(api, planId, taskId, contextKey, async (signal) => {
    const detail = await fetchPlanTaskDetail(api!, planId!, taskId!, signal);
    if (!taskDetailMatchesReference(detail, planId!, taskId!)) {
      throw new Error("Task detail response did not match the selected task.");
    }
    return detail;
  }, undefined, (change, snapshot) => change.reset || change.deleted || !snapshot || change.task_ids.includes(snapshot.task_id));
  return { detail: query.data, loading: query.loading, error: query.error, retry: query.refetch };
}
