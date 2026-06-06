import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../lib/bridge";
import { listen } from "../lib/bridge";
import type { UsageSnapshot, UsageSnapshotEvent } from "../types/usage";

export interface UseUsageSnapshotOptions {
  /** Provider id (e.g. "claude", "codex"). Required — no implicit default
      so adding a new provider doesn't silently fall through to Claude. */
  provider: string;
  accountId?: string | null;
  enabled?: boolean;
}

export interface UseUsageSnapshotResult {
  snapshot: UsageSnapshot | null;
  loading: boolean;
  error: string | null;
  refetch: () => Promise<void>;
  refresh: () => Promise<void>;
}

export function useUsageSnapshot(opts: UseUsageSnapshotOptions): UseUsageSnapshotResult {
  const { provider, accountId = null, enabled = true } = opts;
  const [snapshot, setSnapshot] = useState<UsageSnapshot | null>(null);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const cancelledRef = useRef(false);

  const refetch = useCallback(async () => {
    if (!enabled) return;
    setLoading(true);
    setError(null);
    try {
      const row = await invoke<UsageSnapshot | null>("usage_latest", {
        provider,
        accountId: accountId ?? undefined,
      });
      if (!cancelledRef.current) setSnapshot(row ?? null);
    } catch (e) {
      if (!cancelledRef.current) setError(String(e));
    } finally {
      if (!cancelledRef.current) setLoading(false);
    }
  }, [provider, accountId, enabled]);

  const refresh = useCallback(async () => {
    try {
      await invoke("usage_refresh", { provider, accountId: accountId ?? undefined });
    } catch (e) {
      setError(String(e));
    }
  }, [provider, accountId]);

  useEffect(() => {
    cancelledRef.current = false;
    if (!enabled) return;
    refetch();
    let unlisten: (() => void) | null = null;
    listen<UsageSnapshotEvent>("usage-snapshot-updated", (evt) => {
      if (cancelledRef.current) return;
      if (evt.payload.provider !== provider) return;
      const wantAcc = accountId ?? null;
      if ((evt.payload.account_id ?? null) !== wantAcc) return;
      refetch();
    }).then((fn) => {
      if (cancelledRef.current) fn();
      else unlisten = fn;
    });
    return () => {
      cancelledRef.current = true;
      unlisten?.();
    };
  }, [provider, accountId, enabled, refetch]);

  return { snapshot, loading, error, refetch, refresh };
}

export interface UseUsageHistoryOptions {
  provider: string;
  accountId?: string | null;
  sinceMs: number;
  limit?: number;
  enabled?: boolean;
  refetchToken?: number;
}

export interface UseUsageHistoryResult {
  rows: UsageSnapshot[];
  loading: boolean;
  error: string | null;
  refetch: () => Promise<void>;
}

export function useUsageHistory(opts: UseUsageHistoryOptions): UseUsageHistoryResult {
  const {
    provider,
    accountId = null,
    sinceMs,
    limit = 200,
    enabled = true,
    refetchToken = 0,
  } = opts;
  const [rows, setRows] = useState<UsageSnapshot[]>([]);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const cancelledRef = useRef(false);

  const refetch = useCallback(async () => {
    if (!enabled) return;
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<UsageSnapshot[] | null>("usage_history", {
        provider,
        since: sinceMs,
        limit,
        accountId: accountId ?? undefined,
      });
      if (!cancelledRef.current) setRows(result ?? []);
    } catch (e) {
      if (!cancelledRef.current) setError(String(e));
    } finally {
      if (!cancelledRef.current) setLoading(false);
    }
  }, [provider, accountId, sinceMs, limit, enabled]);

  useEffect(() => {
    cancelledRef.current = false;
    refetch();
    let unlisten: (() => void) | null = null;
    if (enabled) {
      listen<UsageSnapshotEvent>("usage-snapshot-updated", (evt) => {
        if (cancelledRef.current) return;
        if (evt.payload.provider !== provider) return;
        const wantAcc = accountId ?? null;
        if ((evt.payload.account_id ?? null) !== wantAcc) return;
        refetch();
      }).then((fn) => {
        if (cancelledRef.current) fn();
        else unlisten = fn;
      });
    }
    return () => {
      cancelledRef.current = true;
      unlisten?.();
    };
  }, [provider, accountId, enabled, refetch, refetchToken]);

  return { rows, loading, error, refetch };
}

export interface SessionProjection {
  projected_pct: number;
  rate_per_min: number;
  will_exceed: boolean;
}

export interface DayActual {
  day_start_ms: number;
  pct: number | null;
}

export interface WeeklyProjection {
  end_of_today_pct: number;
  end_of_cycle_pct: number;
  avg_weekday_spend: number;
  avg_weekend_spend: number;
  /** Per day-of-week projected spend (0=Sun..6=Sat). */
  dow_estimates: number[];
  rate_per_min_recent: number;
  rest_of_today_est: number;
  today_so_far: number;
  confidence: "high" | "medium" | "low";
  /** Per-day actuals for active cycle, oldest→newest. Authoritative — use this for past + today bars. */
  day_actuals: DayActual[];
}

export interface ProjectionResponse {
  session: SessionProjection | null;
  weekly: WeeklyProjection | null;
}

export interface UseUsageProjectionOptions {
  provider: string;
  accountId?: string | null;
  enabled?: boolean;
}

export interface UseUsageProjectionResult {
  projection: ProjectionResponse | null;
  loading: boolean;
  error: string | null;
  refetch: () => Promise<void>;
}

export function useUsageProjection(opts: UseUsageProjectionOptions): UseUsageProjectionResult {
  const { provider, accountId = null, enabled = true } = opts;
  const [projection, setProjection] = useState<ProjectionResponse | null>(null);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const cancelledRef = useRef(false);

  const refetch = useCallback(async () => {
    if (!enabled) return;
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<ProjectionResponse | null>("usage_projection", {
        provider,
        accountId: accountId ?? undefined,
      });
      if (!cancelledRef.current) setProjection(result ?? null);
    } catch (e) {
      if (!cancelledRef.current) setError(String(e));
    } finally {
      if (!cancelledRef.current) setLoading(false);
    }
  }, [provider, accountId, enabled]);

  useEffect(() => {
    cancelledRef.current = false;
    if (!enabled) return;
    refetch();
    let unlisten: (() => void) | null = null;
    listen<UsageSnapshotEvent>("usage-snapshot-updated", (evt) => {
      if (cancelledRef.current) return;
      if (evt.payload.provider !== provider) return;
      const wantAcc = accountId ?? null;
      if ((evt.payload.account_id ?? null) !== wantAcc) return;
      refetch();
    }).then((fn) => {
      if (cancelledRef.current) fn();
      else unlisten = fn;
    });
    return () => {
      cancelledRef.current = true;
      unlisten?.();
    };
  }, [provider, accountId, enabled, refetch]);

  return { projection, loading, error, refetch };
}
