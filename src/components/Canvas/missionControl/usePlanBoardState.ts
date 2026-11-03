import { useEffect } from "react";
import { useScopedPlanRead } from "../useScopedPlanRead";
import { listen } from "../../../lib/bridge";
import { KPI_RECORDED_EVENT } from "../../../lib/kpiChanges";
import type { ApiConfig } from "../../../lib/types";
import { fetchPlanBoardState, type PlanBoardState } from "./missionControlState";

export interface PlanBoardStateResult {
  state: PlanBoardState | null;
  loading: boolean;
  error: string | null;
}

const PLAN_BOARD_EVENTS = [
  "mem-activity",
  "graph-worker-spawned",
  "graph-worker-progress",
  "graph-worker-done",
  "graph-worker-reported",
  "cron-run-finished",
  "config-changed",
  KPI_RECORDED_EVENT,
] as const;

function planIdFromPayload(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") return null;
  const record = payload as Record<string, unknown>;
  const direct = [record.plan_id, record.planId].find(
    (value): value is string => typeof value === "string" && value.trim().length > 0,
  );
  if (direct) return direct;
  for (const key of ["plan", "source", "worker", "data"] as const) {
    const nested = record[key];
    if (!nested || typeof nested !== "object") continue;
    const nestedRecord = nested as Record<string, unknown>;
    const nestedPlanId = [
      ...(key === "plan" ? [nestedRecord.id] : []),
      nestedRecord.plan_id,
      nestedRecord.planId,
    ].find(
      (value): value is string => typeof value === "string" && value.trim().length > 0,
    );
    if (nestedPlanId) return nestedPlanId;
  }
  return null;
}

function eventTargetsPlan(payload: unknown, planId: string): boolean {
  const eventPlanId = planIdFromPayload(payload);
  return eventPlanId === null || eventPlanId === planId;
}

export function usePlanBoardState(
  api: ApiConfig | undefined,
  planId: string | null,
  revision: number,
): PlanBoardStateResult {
  const query = useScopedPlanRead(api, planId, "mission-control", undefined, async (signal) => {
    const snapshot = await fetchPlanBoardState(api!, planId!, signal);
    if (snapshot.plan_id !== planId) throw new Error("Mission Control response did not match the selected plan.");
    return snapshot;
  }, revision);
  const refresh = query.refetch;

  useEffect(() => {
    if (!api || !planId) return;
    let active = true;
    const unlisteners: Array<() => void> = [];

    const subscribe = async () => {
      try {
        for (const event of PLAN_BOARD_EVENTS) {
          const unlisten = await listen<unknown>(event, (entry) => {
            if (active && eventTargetsPlan(entry.payload, planId)) void refresh();
          });
          if (!active) {
            unlisten();
            return;
          }
          unlisteners.push(unlisten);
        }
      } catch {
        if (!active) return;
      }
    };

    void subscribe();
    return () => {
      active = false;
      for (const unlisten of unlisteners) unlisten();
      unlisteners.length = 0;
    };
  }, [api?.host, api?.port, api?.token, planId, refresh]);

  return { state: query.data, loading: query.loading, error: query.error };
}
