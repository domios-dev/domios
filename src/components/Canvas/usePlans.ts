import type { PlanView as Plan } from "./planProjection";
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { formatPlanFetchError } from "../../lib/fetchResilience";
import { onResync } from "../../lib/resync";
import type { ApiConfig } from "../../lib/types";
import { fetchPlanProjection, fetchPlans, PlanReadError } from "./plansApi";
import { comparePlanRevisions, planRevision, type CommittedPlanChange, type PlanProjection } from "./planProjection";

export interface UsePlansResult {
  plans: Plan[];
  loading: boolean;
  reconnecting: boolean;
  error: string | null;
  refetch: () => Promise<void>;
}

export interface UsePlansOptions {
  chat?: boolean;
  tabId?: string;
  enabled?: boolean;
  project?: string | null;
  contextKey?: number;
}

export function usePlans(api: ApiConfig | undefined, options: UsePlansOptions = {}): UsePlansResult {
  const enabled = (options.enabled ?? true) && Boolean(api?.enabled);
  const key = enabled ? JSON.stringify([api?.host, api?.port, api?.token, options.project, options.chat, options.tabId, options.contextKey]) : null;
  const selected = useRef(key);
  selected.current = key;
  const [state, setState] = useState<{ key: string | null; plans: PlanProjection[]; loading: boolean; error: string | null }>({
    key: null, plans: [], loading: false, error: null,
  });
  const refresh = useRef<() => Promise<void>>(async () => {});
  const refetch = useCallback(() => refresh.current(), []);
  const project = options.project;
  const chat = options.chat;
  const tabId = options.tabId;

  useEffect(() => {
    let active = true;
    let listRead: AbortController | null = null;
    const patches = new Map<string, AbortController>();
    const plans = new Map<string, PlanProjection>();
    const observed = new Map<string, string>();
    const stops: Array<() => void> = [];
    const current = () => active && key !== null && selected.current === key;
    const publish = (error: string | null = null, loading = false) => {
      if (current()) setState({ key, plans: [...plans.values()], loading, error });
    };
    const cancelPatches = () => {
      for (const read of patches.values()) read.abort();
      patches.clear();
    };
    const load = async () => {
      if (!current()) return;
      listRead?.abort();
      cancelPatches();
      const request = new AbortController();
      listRead = request;
      publish(null, plans.size === 0);
      try {
        const result = await fetchPlans(api!, request.signal, { project, chat, tabId });
        if (!current() || request.signal.aborted) return;
        const previous = new Map(plans);
        plans.clear();
        for (const plan of result) {
          const old = previous.get(plan.id);
          plans.set(plan.id, old?.revision === plan.revision ? old : plan);
        }
        publish();
      } catch (cause) {
        if (current() && !request.signal.aborted) {
          if (cause instanceof PlanReadError && [400, 401, 403, 404, 410].includes(cause.status)) plans.clear();
          publish(formatPlanFetchError(cause));
        }
      } finally {
        if (listRead === request) listRead = null;
      }
    };
    refresh.current = load;
    const patch = async (change: CommittedPlanChange) => {
      if (!current()) return;
      const revision = planRevision(change);
      const seen = observed.get(change.plan_id);
      if (seen && comparePlanRevisions(seen, revision) >= 0) return;
      observed.set(change.plan_id, revision);
      const old = plans.get(change.plan_id);
      if (old && comparePlanRevisions(old.revision, revision) >= 0) return;
      if (listRead || tabId) { await load(); return; }
      patches.get(change.plan_id)?.abort();
      const request = new AbortController();
      patches.set(change.plan_id, request);
      if (change.deleted) {
        plans.delete(change.plan_id);
        publish();
      }
      try {
        const plan = await fetchPlanProjection(api!, change.plan_id, chat ? "chat" : "summary", request.signal, project);
        if (!current() || request.signal.aborted) return;
        if (comparePlanRevisions(plan.revision, revision) < 0) throw new Error("Plan projection predates the committed change.");
        plans.set(plan.id, plan);
        publish();
      } catch (cause) {
        if (!current() || request.signal.aborted) return;
        if (cause instanceof PlanReadError && [400, 401, 403, 404, 410].includes(cause.status)) plans.delete(change.plan_id);
        publish(cause instanceof PlanReadError && cause.status === 404 ? null : formatPlanFetchError(cause));
      } finally {
        if (patches.get(change.plan_id) === request) patches.delete(change.plan_id);
      }
    };
    if (key) {
      stops.push(onResync((reason) => { if (reason === "epoch_changed") observed.clear(); void load(); }));
      const boot = () => { observed.clear(); void load(); };
      window.addEventListener("codeterm-web-boot-changed", boot);
      stops.push(() => window.removeEventListener("codeterm-web-boot-changed", boot));
      void Promise.all([
        listen<CommittedPlanChange>("plan-changed", ({ payload }) => { if (current()) void patch(payload).catch(() => load()); }),
        listen("plans-loaded", () => { void load(); }),
      ].map(async (subscription) => {
        const stop = await subscription;
        if (active) stops.push(stop); else stop();
      })).then(() => { if (active) void load(); }).catch((cause: unknown) => publish(formatPlanFetchError(cause)));
    }
    return () => {
      active = false;
      listRead?.abort();
      cancelPatches();
      for (const stop of stops) stop();
    };
  }, [key, api?.host, api?.port, api?.token, project, chat, tabId]);

  const current = state.key === key;
  return {
    plans: key && current ? state.plans : [],
    loading: Boolean(key && (!current || state.loading)),
    reconnecting: Boolean(key && current && state.plans.length && state.error),
    error: key && current ? state.error : null,
    refetch,
  };
}

export { usePlanTaskBoard, type UsePlanTaskBoardResult } from "./usePlanTaskBoard";
export { usePlanTaskDetail, type UsePlanTaskDetailResult } from "./usePlanTaskDetail";
