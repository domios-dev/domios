import type { PlanView as Plan } from "./planProjection";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { formatPlanFetchError } from "../../lib/fetchResilience";
import { peerEventRail } from "../../lib/peerEventRail";
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
  const peerId = api?.peerId;
  const key = enabled ? JSON.stringify([api?.host, api?.port, api?.token, peerId, options.project, options.chat, options.tabId, options.contextKey]) : null;
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
    let listInFlight: Promise<void> | null = null;
    let listQueued = false;
    const patches = new Map<string, AbortController>();
    const pendingPatches = new Map<string, string>();
    const deletedFloors = new Map<string, string>();
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
    const fetchPatch = async (planId: string, opts?: { preempt?: boolean; isDelete?: boolean }) => {
      if (!current()) return;
      if (listRead) {
        if (opts?.isDelete) {
          plans.delete(planId);
          publish();
          await load({ preempt: true });
          return;
        }
        const targetRevision = observed.get(planId);
        if (tabId) listQueued = true;
        else if (targetRevision) pendingPatches.set(planId, targetRevision);
        return;
      }
      if (tabId) {
        if (opts?.isDelete) {
          plans.delete(planId);
          publish();
          await load({ preempt: true });
          return;
        }
        await load();
        return;
      }
      if (patches.has(planId) && !opts?.preempt) {
        const targetRevision = observed.get(planId);
        if (targetRevision) pendingPatches.set(planId, targetRevision);
        return;
      }
      if (opts?.preempt) {
        patches.get(planId)?.abort();
        patches.delete(planId);
        pendingPatches.delete(planId);
      }
      const targetRevision = observed.get(planId);
      if (!targetRevision) return;
      const request = new AbortController();
      patches.set(planId, request);
      if (opts?.isDelete) {
        plans.delete(planId);
        publish();
      }
      try {
        const plan = await fetchPlanProjection(api!, planId, chat ? "chat" : "summary", request.signal, project);
        if (!current() || request.signal.aborted) return;
        const deletedAt = deletedFloors.get(planId);
        if (deletedAt && comparePlanRevisions(plan.revision, deletedAt) <= 0) {
          throw new Error("Plan projection predates a committed deletion.");
        }
        const latestObserved = observed.get(planId) ?? targetRevision;
        if (comparePlanRevisions(plan.revision, targetRevision) < 0) {
          if (comparePlanRevisions(latestObserved, targetRevision) > 0) {
            pendingPatches.set(planId, latestObserved);
            return;
          }
          throw new Error("Plan projection predates the committed change.");
        }
        if (comparePlanRevisions(plan.revision, latestObserved) < 0) {
          pendingPatches.set(planId, latestObserved);
          return;
        }
        if (deletedAt && comparePlanRevisions(plan.revision, deletedAt) > 0) {
          deletedFloors.delete(planId);
        }
        pendingPatches.delete(planId);
        plans.set(plan.id, plan);
        publish();
      } catch (cause) {
        if (!current() || request.signal.aborted) return;
        if (cause instanceof PlanReadError && [400, 401, 403, 404, 410].includes(cause.status)) {
          plans.delete(planId);
          pendingPatches.delete(planId);
        }
        publish(cause instanceof PlanReadError && cause.status === 404 ? null : formatPlanFetchError(cause));
      } finally {
        if (patches.get(planId) === request) {
          patches.delete(planId);
          const followUp = pendingPatches.get(planId);
          if (current() && !request.signal.aborted && !listRead && followUp) {
            pendingPatches.delete(planId);
            const committed = plans.get(planId);
            if (!committed || comparePlanRevisions(committed.revision, followUp) < 0) {
              void fetchPatch(planId);
            }
          }
        }
      }
    };
    const load = async (opts?: { preempt?: boolean }) => {
      if (!current()) return;
      if (listRead && !opts?.preempt) {
        listQueued = true;
        await (listInFlight ?? Promise.resolve());
        return;
      }
      if (opts?.preempt) {
        listQueued = false;
        listRead?.abort();
        cancelPatches();
        pendingPatches.clear();
      } else {
        cancelPatches();
      }
      const execute = async () => {
        for (;;) {
          if (!current()) return;
          const request = new AbortController();
          listRead = request;
          const stalePlanIds: string[] = [];
          let failed = false;
          publish(null, plans.size === 0);
          try {
            const result = await fetchPlans(api!, request.signal, { project, chat, tabId });
            if (!current() || request.signal.aborted) return;
            const previous = new Map(plans);
            plans.clear();
            for (const plan of result) {
              const deletedAt = deletedFloors.get(plan.id);
              if (deletedAt && comparePlanRevisions(plan.revision, deletedAt) <= 0) continue;
              if (deletedAt && comparePlanRevisions(plan.revision, deletedAt) > 0) deletedFloors.delete(plan.id);
              const old = previous.get(plan.id);
              const seen = observed.get(plan.id);
              if (seen && comparePlanRevisions(plan.revision, seen) < 0) {
                if (old && comparePlanRevisions(old.revision, plan.revision) > 0) plans.set(plan.id, old);
                stalePlanIds.push(plan.id);
                continue;
              }
              if (old && comparePlanRevisions(old.revision, plan.revision) > 0) {
                plans.set(plan.id, old);
              } else {
                plans.set(plan.id, old?.revision === plan.revision ? old : plan);
              }
            }
            for (const planId of pendingPatches.keys()) {
              const present = plans.get(planId);
              const seen = observed.get(planId);
              if (seen && (!present || comparePlanRevisions(present.revision, seen) < 0)) {
                if (!stalePlanIds.includes(planId)) stalePlanIds.push(planId);
              }
            }
            publish(null, plans.size === 0 && (listQueued || stalePlanIds.length > 0));
          } catch (cause) {
            failed = true;
            if (current() && !request.signal.aborted) {
              const terminal = cause instanceof PlanReadError && [400, 401, 403, 404, 410].includes(cause.status);
              if (terminal) {
                plans.clear();
                listQueued = false;
              }
              pendingPatches.clear();
              publish(formatPlanFetchError(cause));
            }
          } finally {
            if (listRead === request) listRead = null;
          }
          if (!current() || request.signal.aborted) return;
          if (listQueued) {
            listQueued = false;
            continue;
          }
          if (!failed && stalePlanIds.length > 0) {
            pendingPatches.clear();
            if (tabId) continue;
            for (const planId of stalePlanIds) {
              void fetchPatch(planId);
            }
          } else {
            pendingPatches.clear();
          }
          return;
        }
      };
      const cycle = execute();
      listInFlight = cycle;
      try {
        await cycle;
      } finally {
        if (listInFlight === cycle) listInFlight = null;
      }
    };
    refresh.current = () => load();
    const patch = async (change: CommittedPlanChange) => {
      if (!current()) return;
      const revision = planRevision(change);
      const seen = observed.get(change.plan_id);
      if (seen && comparePlanRevisions(seen, revision) >= 0) return;
      observed.set(change.plan_id, revision);
      if (change.deleted) {
        deletedFloors.set(change.plan_id, revision);
        await fetchPatch(change.plan_id, { preempt: true, isDelete: true });
        return;
      }
      const old = plans.get(change.plan_id);
      if (old && comparePlanRevisions(old.revision, revision) >= 0) return;
      await fetchPatch(change.plan_id);
    };
    if (key) {
      if (peerId) {
        stops.push(peerEventRail.observe(peerId, (event) => {
          if (!current()) return;
          if (event.type === "invalidate") {
            listQueued = false;
            listRead?.abort();
            listRead = null;
            cancelPatches();
            pendingPatches.clear();
            observed.clear();
            deletedFloors.clear();
            plans.clear();
            publish(null, true);
          } else if (event.type === "reconcile" || event.type === "plans-loaded") {
            observed.clear();
            deletedFloors.clear();
            void load({ preempt: true });
          } else if (event.type === "plan-changed") {
            void patch(event.data).catch(() => load());
          }
        }));
        void load({ preempt: true });
      } else {
        stops.push(onResync((reason) => {
          if (reason === "epoch_changed") {
            observed.clear();
            deletedFloors.clear();
          }
          void load({ preempt: true });
        }));
        const boot = () => {
          observed.clear();
          deletedFloors.clear();
          void load({ preempt: true });
        };
        window.addEventListener("codeterm-web-boot-changed", boot);
        stops.push(() => window.removeEventListener("codeterm-web-boot-changed", boot));
        void Promise.all([
          listen<CommittedPlanChange>("plan-changed", ({ payload }) => { if (current()) void patch(payload).catch(() => load()); }),
          listen("plans-loaded", () => { void load(); }),
        ].map(async (subscription) => {
          const stop = await subscription;
          if (active) stops.push(stop); else stop();
        })).then(() => { if (active) void load({ preempt: true }); }).catch((cause: unknown) => publish(formatPlanFetchError(cause)));
      }
    }
    return () => {
      active = false;
      listQueued = false;
      listRead?.abort();
      cancelPatches();
      pendingPatches.clear();
      for (const stop of stops) stop();
    };
  }, [key, api?.host, api?.port, api?.token, peerId, project, chat, tabId]);

  return useMemo(() => {
    const current = state.key === key;
    return {
      plans: key && current ? state.plans : [],
      loading: Boolean(key && (!current || state.loading)),
      reconnecting: Boolean(key && current && state.plans.length && state.error),
      error: key && current ? state.error : null,
      refetch,
    };
  }, [key, state, refetch]);
}

export { usePlanTaskBoard, type UsePlanTaskBoardResult } from "./usePlanTaskBoard";
export { usePlanTaskDetail, type UsePlanTaskDetailResult } from "./usePlanTaskDetail";
