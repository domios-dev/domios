import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../../lib/bridge";
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
  "plan-changed",
  "kpi-recorded",
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
  const [state, setState] = useState<PlanBoardState | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const activeRef = useRef(false);
  const selectionRef = useRef<string | null>(null);
  const queuedRefreshRef = useRef(false);
  const readRef = useRef<AbortController | null>(null);

  const refresh = useCallback(() => {
    if (!api || !planId || !activeRef.current) return;
    if (readRef.current) {
      queuedRefreshRef.current = true;
      return;
    }

    const controller = new AbortController();
    readRef.current = controller;
    setLoading(true);
    fetchPlanBoardState(api, planId, controller.signal)
      .then((next) => {
        if (!activeRef.current || controller.signal.aborted) return;
        setState(next);
        setError(null);
      })
      .catch((e: unknown) => {
        if (!activeRef.current || controller.signal.aborted) return;
        setError(`Mission Control state unavailable: ${String(e)}`);
      })
      .finally(() => {
        if (readRef.current !== controller) return;
        readRef.current = null;
        if (!activeRef.current || controller.signal.aborted) return;
        setLoading(false);
        if (queuedRefreshRef.current) {
          queuedRefreshRef.current = false;
          refresh();
        }
      });
  }, [api, planId]);

  useEffect(() => {
    activeRef.current = true;
    return () => {
      activeRef.current = false;
      queuedRefreshRef.current = false;
      readRef.current?.abort();
      readRef.current = null;
    };
  }, [api, planId]);

  useEffect(() => {
    if (!api || !planId) {
      selectionRef.current = null;
      setState(null);
      setError(null);
      setLoading(false);
      return;
    }
    if (selectionRef.current !== planId) {
      selectionRef.current = planId;
      setState(null);
      setError(null);
    }
    refresh();
  }, [api, planId, refresh, revision]);

  useEffect(() => {
    if (!api || !planId) return;
    let active = true;
    const unlisteners: Array<() => void> = [];

    const subscribe = async () => {
      try {
        for (const event of PLAN_BOARD_EVENTS) {
          const unlisten = await listen<unknown>(event, (entry) => {
            if (active && eventTargetsPlan(entry.payload, planId)) refresh();
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
  }, [api, planId, refresh]);

  return { state, loading, error };
}
