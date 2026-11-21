import { useCallback, useEffect, useRef, useState } from "react";
import { onResync } from "./resync";
import { computeBackoffDelay, formatPlanFetchError, isTransientFetchError } from "./fetchResilience";

export interface UseResilientQueryRefetchOptions {
  preempt?: boolean;
}

export interface UseResilientQueryResult<T> {
  data: T;
  loading: boolean;
  reconnecting: boolean;
  error: string | null;
  refetch: (options?: UseResilientQueryRefetchOptions) => Promise<void>;
  setData: (value: T | ((prev: T) => T)) => void;
}

export interface UseResilientQueryOptions {
  enabled?: boolean;
  deps?: readonly unknown[];
}

function hasItems(data: unknown): boolean {
  return Array.isArray(data) ? data.length > 0 : data != null;
}

function isAuthDeniedError(err: unknown): boolean {
  const status = typeof err === "object" && err !== null && "status" in err ? (err as { status?: unknown }).status : null;
  return status === 401 || status === 403;
}

interface QueryState<T> {
  scopeKey: string | null;
  data: T;
  loading: boolean;
  reconnecting: boolean;
  error: string | null;
}

export function useResilientQuery<T>(
  fetcher: (signal: AbortSignal) => Promise<T>,
  initialData: T,
  options: UseResilientQueryOptions = {},
): UseResilientQueryResult<T> {
  const enabled = options.enabled ?? true;
  const depKey = JSON.stringify(options.deps ?? []);
  const scopeKey = enabled ? depKey : null;
  const initialRef = useRef(initialData);
  const [state, setState] = useState<QueryState<T>>({
    scopeKey,
    data: initialData,
    loading: enabled,
    reconnecting: false,
    error: null,
  });

  const fetcherRef = useRef(fetcher);
  fetcherRef.current = fetcher;
  const hasDataRef = useRef(false);
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const scopeKeyRef = useRef(scopeKey);
  scopeKeyRef.current = scopeKey;
  const authorityRef = useRef(0);
  const ctrlRef = useRef<AbortController | null>(null);
  const inFlightRef = useRef<Promise<void> | null>(null);
  const queuedRef = useRef(false);
  const queuedResetBackoffRef = useRef(false);
  const retryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const attemptRef = useRef(0);
  const consecutiveFailureRef = useRef(0);
  const runRef = useRef<(resetBackoff: boolean, preempt?: boolean) => Promise<void>>(async () => {});

  const clearRetry = useCallback(() => {
    if (retryTimerRef.current) {
      clearTimeout(retryTimerRef.current);
      retryTimerRef.current = null;
    }
  }, []);

  const setData = useCallback((value: T | ((prev: T) => T)) => {
    setState((prev) => {
      const currentScope = scopeKeyRef.current;
      const base = prev.scopeKey === currentScope ? prev.data : initialRef.current;
      const next = typeof value === "function" ? (value as (p: T) => T)(base) : value;
      hasDataRef.current = hasItems(next);
      return { ...prev, scopeKey: currentScope, data: next };
    });
  }, []);

  const scheduleRetry = useCallback(() => {
    clearRetry();
    const delay = computeBackoffDelay(++attemptRef.current);
    retryTimerRef.current = setTimeout(() => {
      retryTimerRef.current = null;
      void runRef.current(false, false);
    }, delay);
  }, [clearRetry]);

  const run = useCallback(
    async (resetBackoff: boolean, preempt = false) => {
      const activeScope = scopeKeyRef.current;
      if (!enabledRef.current || activeScope === null) return;
      if (ctrlRef.current && !preempt) {
        queuedRef.current = true;
        if (resetBackoff) queuedResetBackoffRef.current = true;
        await (inFlightRef.current ?? Promise.resolve());
        return;
      }
      if (preempt) {
        queuedRef.current = false;
        queuedResetBackoffRef.current = false;
        ctrlRef.current?.abort();
      }
      const runAuthority = authorityRef.current;
      const execute = async (initialResetBackoff: boolean) => {
        let shouldResetBackoff = initialResetBackoff;
        for (;;) {
          if (!enabledRef.current || authorityRef.current !== runAuthority || scopeKeyRef.current !== activeScope) {
            return;
          }
          if (shouldResetBackoff) {
            attemptRef.current = 0;
            clearRetry();
          }

          const ctrl = new AbortController();
          ctrlRef.current = ctrl;

          const stale = hasDataRef.current;
          setState((prev) => {
            const sameScope = prev.scopeKey === activeScope;
            return {
              scopeKey: activeScope,
              data: sameScope ? prev.data : initialRef.current,
              loading: !stale,
              reconnecting: sameScope ? prev.reconnecting : false,
              error: null,
            };
          });

          try {
            const next = await fetcherRef.current(ctrl.signal);
            if (ctrl.signal.aborted || authorityRef.current !== runAuthority || scopeKeyRef.current !== activeScope) {
              return;
            }
            hasDataRef.current = hasItems(next);
            attemptRef.current = 0;
            consecutiveFailureRef.current = 0;
            setState({
              scopeKey: activeScope,
              data: next,
              loading: false,
              reconnecting: false,
              error: null,
            });
          } catch (e) {
            if (
              ctrl.signal.aborted
              || authorityRef.current !== runAuthority
              || scopeKeyRef.current !== activeScope
              || (e instanceof Error && e.name === "AbortError")
            ) {
              return;
            }
            const friendly = formatPlanFetchError(e);
            if (isAuthDeniedError(e)) {
              hasDataRef.current = false;
              consecutiveFailureRef.current = 0;
              queuedRef.current = false;
              queuedResetBackoffRef.current = false;
              setState({
                scopeKey: activeScope,
                data: initialRef.current,
                loading: false,
                reconnecting: false,
                error: friendly,
              });
              return;
            }
            if (!hasDataRef.current) {
              setState({
                scopeKey: activeScope,
                data: initialRef.current,
                loading: false,
                reconnecting: false,
                error: friendly,
              });
            } else {
              consecutiveFailureRef.current += 1;
              const reconnectingNow = consecutiveFailureRef.current >= 2;
              setState((prev) => (
                prev.scopeKey === activeScope
                  ? { ...prev, loading: false, reconnecting: reconnectingNow }
                  : prev
              ));
            }
            if (!queuedRef.current && (hasDataRef.current || isTransientFetchError(e))) {
              scheduleRetry();
            }
          } finally {
            if (ctrlRef.current === ctrl) {
              ctrlRef.current = null;
            }
          }

          if (
            authorityRef.current !== runAuthority
            || !enabledRef.current
            || scopeKeyRef.current !== activeScope
            || !queuedRef.current
          ) {
            return;
          }
          shouldResetBackoff = queuedResetBackoffRef.current;
          queuedRef.current = false;
          queuedResetBackoffRef.current = false;
        }
      };

      const cycle = execute(resetBackoff);
      inFlightRef.current = cycle;
      try {
        await cycle;
      } finally {
        if (inFlightRef.current === cycle) {
          inFlightRef.current = null;
        }
      }
    },
    [clearRetry, scheduleRetry],
  );

  runRef.current = run;

  const refetch = useCallback(async (opts?: UseResilientQueryRefetchOptions) => {
    await run(true, Boolean(opts?.preempt));
  }, [run]);

  useEffect(() => {
    authorityRef.current += 1;
    queuedRef.current = false;
    queuedResetBackoffRef.current = false;
    hasDataRef.current = false;
    consecutiveFailureRef.current = 0;
    attemptRef.current = 0;
    ctrlRef.current?.abort();
    ctrlRef.current = null;
    inFlightRef.current = null;
    clearRetry();

    if (!enabled) {
      setState({
        scopeKey: null,
        data: initialRef.current,
        loading: false,
        reconnecting: false,
        error: null,
      });
      return;
    }

    setState({
      scopeKey,
      data: initialRef.current,
      loading: true,
      reconnecting: false,
      error: null,
    });
    void run(true, true);
    return () => {
      authorityRef.current += 1;
      queuedRef.current = false;
      queuedResetBackoffRef.current = false;
      ctrlRef.current?.abort();
      ctrlRef.current = null;
      inFlightRef.current = null;
      clearRetry();
    };
  }, [enabled, clearRetry, run, scopeKey]);

  useEffect(() => {
    if (!enabled) return;
    const unResync = onResync(() => {
      void run(true, true);
    });
    const onBoot = () => {
      void run(true, true);
    };
    window.addEventListener("codeterm-web-boot-changed", onBoot);
    return () => {
      unResync();
      window.removeEventListener("codeterm-web-boot-changed", onBoot);
    };
  }, [enabled, run]);

  const current = scopeKey !== null && state.scopeKey === scopeKey;
  return {
    data: current ? state.data : initialRef.current,
    loading: Boolean(scopeKey !== null && (!current || state.loading)),
    reconnecting: Boolean(current && state.reconnecting),
    error: current ? state.error : null,
    refetch,
    setData,
  };
}
