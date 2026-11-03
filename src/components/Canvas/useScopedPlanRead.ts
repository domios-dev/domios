import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { formatPlanFetchError } from "../../lib/fetchResilience";
import { onResync } from "../../lib/resync";
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
  const key = api && planId && entityKey
    ? JSON.stringify([api.host, api.port, api.token, planId, entityKey, contextKey]) : null;
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
    const stops: Array<() => void> = [];
    let snapshot = latest.current.key === key ? latest.current.data : null;
    let minimumRevision = snapshot ? planRevision(snapshot) : "0";
    let observedRevision = minimumRevision;
    const run = async () => {
      if (!active || !key || selected.current !== key) return;
      controller?.abort();
      const request = new AbortController();
      controller = request;
      version.current++;
      setState((previous) => ({
        key, data: previous.key === key ? previous.data : null, loading: true, error: null,
      }));
      try {
        const data = await reader.current(request.signal);
        if (request.signal.aborted) return;
        const revision = planRevision(data);
        if (comparePlanRevisions(revision, minimumRevision) < 0) throw new Error("Plan projection predates a committed change.");
        if (active && !request.signal.aborted && selected.current === key) {
          snapshot = data;
          minimumRevision = revision;
          if (comparePlanRevisions(revision, observedRevision) > 0) observedRevision = revision;
          setState({ key, data, loading: false, error: null });
        }
      } catch (cause) {
        if (!active || request.signal.aborted || selected.current !== key) return;
        const unavailable = cause instanceof PlanReadError && [400, 401, 403, 404, 410].includes(cause.status);
        if (unavailable) snapshot = null;
        setState((previous) => ({
          key, data: !unavailable && previous.key === key ? previous.data : null,
          loading: false, error: formatPlanFetchError(cause),
        }));
      }
    };
    refresh.current = run;
    if (key) {
      const recover = () => {
        void run();
      };
      const reboot = () => {
        minimumRevision = "0";
        observedRevision = "0";
        void run();
      };
      stops.push(onResync((reason) => reason === "epoch_changed" ? reboot() : recover()));
      window.addEventListener("codeterm-web-boot-changed", reboot);
      stops.push(() => window.removeEventListener("codeterm-web-boot-changed", reboot));
      const changed = (change: CommittedPlanChange) => {
        if (change.plan_id !== planId) return;
        const revision = planRevision(change);
        if (comparePlanRevisions(revision, observedRevision) <= 0) return;
        const gap = change.previous_revision !== observedRevision;
        observedRevision = revision;
        if (!gap && target.current && !target.current(change, snapshot)) return;
        minimumRevision = revision;
        void run();
      };
      void Promise.all([
        listen<CommittedPlanChange>("plan-changed", ({ payload }) => {
          try { changed(payload); } catch { recover(); }
        }),
        listen("plans-loaded", recover),
      ].map(async (subscription) => {
        const stop = await subscription;
        if (active) stops.push(stop); else stop();
      })).then(() => { if (active) void run(); }).catch((cause: unknown) => {
        if (active) setState({ key, data: null, loading: false, error: formatPlanFetchError(cause) });
      });
    }
    return () => {
      active = false;
      controller?.abort();
      version.current++;
      for (const stop of stops) stop();
    };
  }, [key, planId, refreshKey]);

  const current = state.key === key;
  return {
    data: key && current ? state.data : null,
    loading: Boolean(key && (!current || state.loading)),
    error: key && current ? state.error : null,
    refetch, setData, version,
  };
}
