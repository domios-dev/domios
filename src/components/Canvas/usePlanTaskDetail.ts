import { useCallback, useEffect, useRef, useState } from "react";
import { formatPlanFetchError } from "../../lib/fetchResilience";
import { planRevision } from "./planProjection";
import type { ApiConfig } from "../../lib/types";
import { fetchPlanTaskDetail, fetchPlanTaskHistory, PlanReadError } from "./plansApi";
import { taskDisplayKey } from "./taskDisplayKey";
import type { PlanTaskDetail, TaskHistoryKind } from "./types";
import { useScopedPlanRead } from "./useScopedPlanRead";

export interface UsePlanTaskDetailResult {
  detail: PlanTaskDetail | null;
  loading: boolean;
  error: string | null;
  retry: () => Promise<void>;
  loadHistory: (kind: TaskHistoryKind) => Promise<void>;
  historyLoading: (kind: TaskHistoryKind) => boolean;
  historyError: (kind: TaskHistoryKind) => string | null;
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
  const controllers = useRef(new Map<TaskHistoryKind, AbortController>());
  const [loading, setLoading] = useState<ReadonlySet<TaskHistoryKind>>(new Set());
  const [errors, setErrors] = useState<ReadonlyMap<TaskHistoryKind, string>>(new Map());
  useEffect(() => {
    const reads = controllers.current;
    for (const controller of reads.values()) controller.abort();
    reads.clear(); setLoading(new Set()); setErrors(new Map());
    return () => { for (const controller of reads.values()) controller.abort(); reads.clear(); };
  }, [api?.host, api?.port, api?.token, planId, taskId, contextKey, query.loading]);
  const { data, setData, version, refetch } = query;
  const loadHistory = useCallback(async (kind: TaskHistoryKind) => {
    const before = data?.history?.[kind];
    if (!api || !data || before === null || before === undefined || query.loading || controllers.current.has(kind)) return;
    const request = new AbortController();
    const generation = version.current;
    controllers.current.set(kind, request);
    setLoading((previous) => new Set(previous).add(kind));
    setErrors((previous) => { const next = new Map(previous); next.delete(kind); return next; });
    try {
      const page = await fetchPlanTaskHistory(api, data.plan_id, data.task_id, kind, before, request.signal);
      if (request.signal.aborted || version.current !== generation) return;
      if (planRevision(page) !== planRevision(data)) { await refetch(); return; }
      setData((current) => {
        if (!current || current.task_id !== data.task_id || planRevision(current) !== page.revision || current.history?.[kind] !== before) return current;
        const task = { ...current.task };
        if (kind === "milestones") task.milestones = [...page.items as PlanTaskDetail["task"]["milestones"], ...task.milestones];
        else task.execution_attempts = [...page.items as PlanTaskDetail["task"]["execution_attempts"], ...(task.execution_attempts ?? [])];
        return { ...current, task, history: { ...current.history!, [kind]: page.next_cursor } };
      });
    } catch (cause) {
      if (request.signal.aborted || version.current !== generation) return;
      if (cause instanceof PlanReadError && [400,401,403,404,410].includes(cause.status)) await refetch();
      else setErrors((previous) => new Map(previous).set(kind, formatPlanFetchError(cause)));
    } finally {
      if (controllers.current.get(kind) === request) {
        controllers.current.delete(kind);
        setLoading((previous) => { const next = new Set(previous); next.delete(kind); return next; });
      }
    }
  }, [api?.host, api?.port, api?.token, data, query.loading, version, setData, refetch]);
  return { detail: data, loading: query.loading, error: query.error, retry: refetch, loadHistory,
    historyLoading: (kind) => loading.has(kind), historyError: (kind) => errors.get(kind) ?? null };

}
