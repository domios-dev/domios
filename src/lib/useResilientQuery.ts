import { useCallback, useEffect, useRef, useState } from "react";
import { onResync } from "./resync";
import { computeBackoffDelay, formatPlanFetchError, isTransientFetchError } from "./fetchResilience";

export interface UseResilientQueryResult<T> {
  data: T;
  loading: boolean;
  reconnecting: boolean;
  error: string | null;
  refetch: () => Promise<void>;
  setData: (value: T | ((prev: T) => T)) => void;
}

export interface UseResilientQueryOptions {
  enabled?: boolean;
  deps?: readonly unknown[];
}

function hasItems(data: unknown): boolean {
  return Array.isArray(data) ? data.length > 0 : data != null;
}

export function useResilientQuery<T>(
  fetcher: (signal: AbortSignal) => Promise<T>,
  initialData: T,
  options: UseResilientQueryOptions = {},
): UseResilientQueryResult<T> {
  const enabled = options.enabled ?? true;
  const initialRef = useRef(initialData);
  const [data, setDataState] = useState<T>(initialData);
  const [loading, setLoading] = useState(enabled);
  const [reconnecting, setReconnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetcherRef = useRef(fetcher);
  fetcherRef.current = fetcher;
  const hasDataRef = useRef(false);
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const ctrlRef = useRef<AbortController | null>(null);
  const retryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const attemptRef = useRef(0);
  const consecutiveFailureRef = useRef(0);
  const runRef = useRef<(resetBackoff: boolean) => Promise<void>>(async () => {});

  const clearRetry = useCallback(() => {
    if (retryTimerRef.current) {
      clearTimeout(retryTimerRef.current);
      retryTimerRef.current = null;
    }
  }, []);

  const setData = useCallback((value: T | ((prev: T) => T)) => {
    setDataState((prev) => {
      const next = typeof value === "function" ? (value as (p: T) => T)(prev) : value;
      hasDataRef.current = hasItems(next);
      return next;
    });
  }, []);

  const scheduleRetry = useCallback(() => {
    clearRetry();
    const delay = computeBackoffDelay(++attemptRef.current);
    retryTimerRef.current = setTimeout(() => {
      retryTimerRef.current = null;
      void runRef.current(false);
    }, delay);
  }, [clearRetry]);

  const run = useCallback(
    async (resetBackoff: boolean) => {
      if (!enabledRef.current) return;
      if (resetBackoff) {
        attemptRef.current = 0;
        clearRetry();
      }

      ctrlRef.current?.abort();
      const ctrl = new AbortController();
      ctrlRef.current = ctrl;

      const stale = hasDataRef.current;
      if (!stale) setLoading(true);
      setError(null);

      try {
        const next = await fetcherRef.current(ctrl.signal);
        if (ctrl.signal.aborted) return;
        setDataState(next);
        hasDataRef.current = hasItems(next);
        setError(null);
        attemptRef.current = 0;
        consecutiveFailureRef.current = 0;
        setReconnecting(false);
      } catch (e) {
        if (ctrl.signal.aborted || (e instanceof Error && e.name === "AbortError")) return;
        const friendly = formatPlanFetchError(e);
        if (!hasDataRef.current) setError(friendly);
        if (hasDataRef.current) {
          consecutiveFailureRef.current += 1;
          if (consecutiveFailureRef.current >= 2) setReconnecting(true);
        }
        if (hasDataRef.current || isTransientFetchError(e)) scheduleRetry();
      } finally {
        if (!ctrl.signal.aborted) {
          setLoading(false);
        }
      }
    },
    [clearRetry, scheduleRetry],
  );

  runRef.current = run;

  const refetch = useCallback(async () => {
    await run(true);
  }, [run]);

  const depKey = JSON.stringify(options.deps ?? []);

  useEffect(() => {
    if (!enabled) {
      setLoading(false);
      setReconnecting(false);
      setError(null);
      hasDataRef.current = false;
      consecutiveFailureRef.current = 0;
      setDataState(initialRef.current);
      ctrlRef.current?.abort();
      clearRetry();
      return;
    }
    void run(true);
    return () => {
      ctrlRef.current?.abort();
      clearRetry();
    };
  }, [enabled, clearRetry, run, depKey]);

  useEffect(() => {
    if (!enabled) return;
    const onRecovery = () => {
      void run(true);
    };
    const unResync = onResync(onRecovery);
    const onBoot = () => onRecovery();
    window.addEventListener("codeterm-web-boot-changed", onBoot);
    return () => {
      unResync();
      window.removeEventListener("codeterm-web-boot-changed", onBoot);
    };
  }, [enabled, run]);

  return { data, loading, reconnecting, error, refetch, setData };
}
