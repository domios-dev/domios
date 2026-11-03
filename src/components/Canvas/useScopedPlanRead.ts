import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { formatPlanFetchError } from "../../lib/fetchResilience";
import { onResync } from "../../lib/resync";
import type { ApiConfig } from "../../lib/types";
import { PlanReadError } from "./plansApi";

export function useScopedPlanRead<T>(
  api: ApiConfig | undefined,
  planId: string | null,
  entityKey: string | null,
  contextKey: number | undefined,
  read: (signal: AbortSignal) => Promise<T>,
  refreshKey?: number,
) {
  const key = api && planId && entityKey
    ? JSON.stringify([api.host, api.port, api.token, planId, entityKey, contextKey]) : null;
  const selected = useRef(key);
  selected.current = key;
  const reader = useRef(read);
  reader.current = read;
  const version = useRef(0);
  const [state, setState] = useState<{ key: string | null; data: T | null; loading: boolean; error: string | null }>({
    key: null, data: null, loading: false, error: null,
  });
  const refresh = useRef<() => Promise<void>>(async () => {});
  const refetch = useCallback(() => refresh.current(), []);
  const setData = useCallback((update: (previous: T | null) => T | null) => {
    setState((previous) => previous.key === selected.current ? { ...previous, data: update(previous.data) } : previous);
  }, []);

  useEffect(() => {
    let active = true;
    let controller: AbortController | null = null;
    const stops: Array<() => void> = [];
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
        if (active && !request.signal.aborted && selected.current === key) {
          setState({ key, data, loading: false, error: null });
        }
      } catch (cause) {
        if (!active || request.signal.aborted || selected.current !== key) return;
        const unavailable = cause instanceof PlanReadError && [401, 403, 404].includes(cause.status);
        setState((previous) => ({
          key, data: !unavailable && previous.key === key ? previous.data : null,
          loading: false, error: formatPlanFetchError(cause),
        }));
      }
    };
    refresh.current = run;
    if (key) {
      stops.push(onResync(() => { void run(); }));
      const onBoot = () => { void run(); };
      window.addEventListener("codeterm-web-boot-changed", onBoot);
      stops.push(() => window.removeEventListener("codeterm-web-boot-changed", onBoot));
      void Promise.all(["plan-changed", "plans-loaded"].map(async (event) => {
        const stop = await listen<{ plan_id?: string }>(event, ({ payload }) => {
          if (!payload?.plan_id || payload.plan_id === planId) void run();
        });
        if (active) stops.push(stop);
        else stop();
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
