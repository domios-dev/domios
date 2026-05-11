import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { UsageSnapshot, UsageSnapshotEvent } from "../types/usage";

export interface UseUsageSnapshotOptions {
  provider?: string;
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

export function useUsageSnapshot(opts: UseUsageSnapshotOptions = {}): UseUsageSnapshotResult {
  const { provider = "claude", accountId = null, enabled = true } = opts;
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
  provider?: string;
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
    provider = "claude",
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
