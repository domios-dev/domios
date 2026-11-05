import { useEffect, useMemo, useRef, useState } from "react";
import { PlanReadError } from "../plansApi";
import { planRevision } from "../planProjection";
import type { EventPager } from "./missionControlBoardModel";
import type { PlanEvent } from "./missionControlState";
import { formatPlanFetchError } from "../../../lib/fetchResilience";
import { useScopedPlanRead } from "../useScopedPlanRead";
import { listen } from "../../../lib/bridge";
import { KPI_RECORDED_EVENT } from "../../../lib/kpiChanges";
import type { ApiConfig } from "../../../lib/types";
import { fetchPlanBoardState, type PlanBoardState } from "./missionControlState";

export interface PlanBoardStateResult {
  state: PlanBoardState | null;
  loading: boolean;
  error: string | null;
  events?: EventPager;
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
  includeEvents = true,
): PlanBoardStateResult {
  const query = useScopedPlanRead(api, planId, "mission-control", undefined, async (signal) => {
    const snapshot = await fetchPlanBoardState(api!, planId!, signal, undefined, includeEvents);
    if (snapshot.plan_id !== planId) throw new Error("Mission Control response did not match the selected plan.");
    return snapshot;
  }, revision * 2 + Number(includeEvents));
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

  const snapshot = query.data;
  const key = JSON.stringify([api?.host, api?.port, api?.token, planId, snapshot?.revision]);
  const selected = useRef(key); selected.current = key;
  const pending = useRef<AbortController | null>(null);
  const [history, setHistory] = useState<{ key: string; pages: PlanEvent[][]; cursor: number | null; error: string | null } | null>(null);
  useEffect(() => {
    pending.current?.abort(); pending.current = null;
    return () => { pending.current?.abort(); pending.current = null; };
  }, [key, query.loading]);
  const events = useMemo<EventPager | undefined>(() => {
    if (!snapshot) return undefined;
    if(snapshot.event_window_loaded===false)return {pageCount:1,page:()=>null,caption:`${snapshot.mission_control.event_log.recorded_events} events`};
    const current = history?.key === key ? history : null;
    const pages = [snapshot.mission_control.event_log.entries.slice().reverse(), ...(current?.pages ?? [])];
    const cursor = current ? current.cursor : snapshot.next_event_cursor ?? null;
    return {
      pageCount: pages.length + (cursor === null ? 0 : 1),
      page: (index) => pages[index] ?? null,
      caption: `${snapshot.mission_control.event_log.recorded_events} events`,
      error: current?.error,
      load: (index) => {
        if (!api || !planId || cursor === null || index !== pages.length || pending.current || query.loading) return;
        const controller = new AbortController(); pending.current = controller;
        const generation = query.version.current;
        setHistory({ key, pages: current?.pages ?? [], cursor, error: null });
        void fetchPlanBoardState(api, planId, controller.signal, cursor).then(async (page) => {
          if (controller.signal.aborted || selected.current !== key || query.version.current !== generation) return;
          if (page.plan_id !== planId || planRevision(page) !== planRevision(snapshot)) { await refresh(); return; }
          setHistory({ key, pages: [...(current?.pages ?? []), page.mission_control.event_log.entries.slice().reverse()], cursor: page.next_event_cursor ?? null, error: null });
        }).catch(async (cause) => {
          if(controller.signal.aborted || selected.current!==key || query.version.current!==generation)return;
          if (cause instanceof PlanReadError && [400,401,403,404,410].includes(cause.status)) { await refresh(); return; }
          if (!controller.signal.aborted && selected.current === key && query.version.current === generation)
            setHistory({ key, pages: current?.pages ?? [], cursor, error: formatPlanFetchError(cause) });
        }).finally(() => { if (pending.current === controller) pending.current = null; });
      },
    };
  }, [api?.host, api?.port, api?.token, planId, key, snapshot, history, query.loading, query.version, refresh]);
  return { state: snapshot, loading: query.loading, error: query.error, events };

}

export function useDeferredPlanBoardState(
  api: ApiConfig | undefined,
  planId: string | null,
  revision: number,
  includeEvents: boolean,
): PlanBoardStateResult {
  const summary = usePlanBoardState(api, planId, revision, false);
  const eventRead = usePlanBoardState(api, includeEvents && summary.state ? planId : null, revision);
  const currentEvents = eventRead.state && summary.state
    && planRevision(eventRead.state) === planRevision(summary.state) ? eventRead.events : undefined;
  const pager = currentEvents ?? summary.events;
  return {
    ...summary,
    events: pager && { ...pager, error: eventRead.error ?? pager.error },
  };
}
