import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { formatPlanFetchError } from "../../lib/fetchResilience";
import { onResync } from "../../lib/resync";
import { peerEventRail } from "../../lib/peerEventRail";
import type { ApiConfig } from "../../lib/types";
import { PlanReadError } from "./plansApi";
import { comparePlanRevisions, planRevision, type CommittedPlanChange } from "./planProjection";

export function useScopedPlanRead<T>(
  api: ApiConfig | undefined,
  planId: string | null,
  entityKey: string | null,
  contextKey: number | undefined,
  read: (signal: AbortSignal) => Promise<T>,
  refreshKey?: number,
  targets?: (change: CommittedPlanChange, snapshot: T | null) => boolean,
) {
  const peerId = api?.peerId;
  const key = api && planId && entityKey
    ? JSON.stringify([api.host, api.port, api.token, peerId, planId, entityKey, contextKey]) : null;
  const selected = useRef(key);
  selected.current = key;
  const reader = useRef(read);
  reader.current = read;
  const target = useRef(targets);
  target.current = targets;
  const version = useRef(0);
  const [state, setState] = useState<{ key: string | null; data: T | null; loading: boolean; error: string | null }>({
    key: null, data: null, loading: false, error: null,
  });
  const latest = useRef(state);
  latest.current = state;
  const refresh = useRef<() => Promise<void>>(async () => {});
  const refetch = useCallback(() => refresh.current(), []);
  const setData = useCallback((update: (previous: T | null) => T | null) => {
    setState((previous) => previous.key === selected.current ? { ...previous, data: update(previous.data) } : previous);
  }, []);

  useEffect(() => {
    let active = true;
    let controller: AbortController | null = null;
    let inFlight: Promise<void> | null = null;
    let pendingReason: "none" | "revision" | "refresh" = "none";
    const stops: Array<() => void> = [];
    let snapshot = latest.current.key === key ? latest.current.data : null;
    let minimumRevision = snapshot ? planRevision(snapshot) : "0";
    let observedRevision = minimumRevision;
    const run = async (options: { preempt?: boolean; reason?: "revision" | "refresh" } = {}) => {
      if (!active || !key || selected.current !== key) return;
      const reason = options.reason ?? "refresh";
      if (controller && !options.preempt) {
        pendingReason = pendingReason === "refresh" || reason === "refresh" ? "refresh" : "revision";
        await (inFlight ?? Promise.resolve());
        return;
      }
      if (options.preempt) {
        pendingReason = "none";
        controller?.abort();
      }
      const execute = async () => {
        for (;;) {
          if (!active || !key || selected.current !== key) return;
          const request = new AbortController();
          controller = request;
          const requiredRevision = minimumRevision;
          version.current++;
          setState((previous) => ({
            key, data: previous.key === key ? previous.data : null, loading: true, error: null,
          }));
          try {
            const data = await reader.current(request.signal);
            if (!active || request.signal.aborted || selected.current !== key) return;
            const revision = planRevision(data);
            if (comparePlanRevisions(revision, minimumRevision) < 0) {
              if (comparePlanRevisions(minimumRevision, requiredRevision) > 0) {
                pendingReason = "none";
                continue;
              }
              throw new Error("Plan projection predates a committed change.");
            }
            snapshot = data;
            minimumRevision = revision;
            if (comparePlanRevisions(revision, observedRevision) > 0) observedRevision = revision;
            if (pendingReason === "revision" && comparePlanRevisions(revision, observedRevision) >= 0) {
              pendingReason = "none";
            }
            setState({ key, data, loading: pendingReason !== "none", error: null });
          } catch (cause) {
            if (!active || request.signal.aborted || selected.current !== key) return;
            const unavailable = cause instanceof PlanReadError && [400, 401, 403, 404, 410].includes(cause.status);
            if (unavailable) {
              snapshot = null;
              pendingReason = "none";
            }
            if (!unavailable && pendingReason !== "none") {
              pendingReason = "none";
              continue;
            }
            setState((previous) => ({
              key, data: !unavailable && previous.key === key ? previous.data : null,
              loading: false, error: formatPlanFetchError(cause),
            }));
          } finally {
            if (controller === request) controller = null;
          }
          if (!active || !key || selected.current !== key || request.signal.aborted) return;
          if (pendingReason !== "none") {
            pendingReason = "none";
            continue;
          }
          return;
        }
      };
      const cycle = execute();
      inFlight = cycle;
      try {
        await cycle;
      } finally {
        if (inFlight === cycle) inFlight = null;
      }
    };
    refresh.current = () => run({ preempt: false, reason: "refresh" });
    if (key) {
      const recover = () => {
        void run({ preempt: true, reason: "refresh" });
      };
      const reboot = () => {
        minimumRevision = "0";
        observedRevision = "0";
        void run({ preempt: true, reason: "refresh" });
      };
      const changed = (change: CommittedPlanChange) => {
        if (change.plan_id !== planId) return;
        const revision = planRevision(change);
        if (comparePlanRevisions(revision, observedRevision) <= 0) return;
        const gap = change.previous_revision !== observedRevision;
        observedRevision = revision;
        if (change.deleted) {
          minimumRevision = revision;
          snapshot = null;
          pendingReason = "none";
          void run({ preempt: true, reason: "revision" });
          return;
        }
        if (!gap && target.current && !target.current(change, snapshot)) return;
        minimumRevision = revision;
        void run({ preempt: false, reason: "revision" });
      };
      if (peerId) {
        stops.push(peerEventRail.observe(peerId, (event) => {
          if (event.type === "invalidate") {
            controller?.abort();
            controller = null;
            pendingReason = "none";
            snapshot = null;
            minimumRevision = observedRevision = "0";
            setState({ key, data: null, loading: true, error: null });
          } else if (event.type === "reconcile" || event.type === "plans-loaded") {
            reboot();
          } else if (event.type === "plan-changed") {
            try { changed(event.data); } catch { recover(); }
          }
        }));
        void run({ preempt: true, reason: "refresh" });
      } else {
        stops.push(onResync((reason) => reason === "epoch_changed" ? reboot() : recover()));
        window.addEventListener("codeterm-web-boot-changed", reboot);
        stops.push(() => window.removeEventListener("codeterm-web-boot-changed", reboot));
        void Promise.all([
          listen<CommittedPlanChange>("plan-changed", ({ payload }) => {
            try { changed(payload); } catch { recover(); }
          }),
          listen("plans-loaded", recover),
        ].map(async (subscription) => {
          const stop = await subscription;
          if (active) stops.push(stop); else stop();
        })).then(() => { if (active) void run({ preempt: true, reason: "refresh" }); }).catch((cause: unknown) => {
          if (active) setState({ key, data: null, loading: false, error: formatPlanFetchError(cause) });
        });
      }
    }
    return () => {
      active = false;
      pendingReason = "none";
      controller?.abort();
      version.current++;
      for (const stop of stops) stop();
    };
  }, [key, peerId, planId]);

  const lastRefresh = useRef<{ key: string | null; value: number | undefined }>({ key, value: refreshKey });
  useEffect(() => {
    const previous = lastRefresh.current;
    lastRefresh.current = { key, value: refreshKey };
    if (!key || previous.key !== key || previous.value === refreshKey) return;
    void refresh.current();
  }, [key, refreshKey]);

  const current = state.key === key;
  return {
    data: key && current ? state.data : null,
    loading: Boolean(key && (!current || state.loading)),
    error: key && current ? state.error : null,
    refetch, setData, version,
  };
}
