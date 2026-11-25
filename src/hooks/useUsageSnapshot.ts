import { useCallback, useEffect, useRef, useState } from "react";
import { readUsageOnEvent } from "../lib/usageEventReads";
import { DEV_BUILD } from "../lib/devBuild";
import { invoke } from "../lib/bridge";
import { scopedUsageArgs, subscribeUsageUpdates, useUsageMachine } from "./usageMachine";
import type { UsageSnapshot, UsageSnapshotEvent, UsageLatestView } from "../types/usage";
import { nextResetAt, zeroExpiredResets } from "../components/Usage/cycle";

// reset still fires without depending on one giant timer surviving that long.
const MAX_RESET_DELAY_MS = 6 * 60 * 60 * 1000;

export interface UseUsageSnapshotOptions {
  /** Provider id (e.g. "claude", "codex"). Required — no implicit default
      so adding a new provider doesn't silently fall through to Claude. */
  provider: string;
  accountId?: string | null;
  enabled?: boolean;
}

export interface UseUsageSnapshotResult {
  snapshot: UsageLatestView | null;
  loading: boolean;
  error: string | null;
  refetch: () => Promise<void>;
  refresh: () => Promise<void>;
}

export function useUsageSnapshot(opts: UseUsageSnapshotOptions): UseUsageSnapshotResult {
  const { provider, accountId = null, enabled = true } = opts;
  const peerId = useUsageMachine();
  const scope = JSON.stringify([peerId, provider, accountId]);
  const [reading, setReading] = useState<{ scope: string; snapshot: UsageLatestView | null } | null>(null);
  const [loadingScope, setLoadingScope] = useState<string | null>(null);
  const [readError, setReadError] = useState<{ scope: string; message: string } | null>(null);
  // Held apart from read errors: the read that follows a failed refresh must not erase its cause.
  const [refreshError, setRefreshError] = useState<{ scope: string; message: string } | null>(null);
  const [resetTick, setResetTick] = useState(0);
  const cancelledRef = useRef(false);
  const readSequence = useRef(0);
  const snapshot = enabled && reading?.scope === scope ? reading.snapshot : null;
  const snapshotKey = snapshot ? JSON.stringify(snapshot) : null;

  const refetch = useCallback(async (event?: UsageSnapshotEvent) => {
    if (!enabled) return;
    const read = ++readSequence.current;
    setLoadingScope(scope);
    setReadError(null);
    try {
      const row = await readUsageOnEvent<UsageLatestView | null>(event, "usage_latest", {
        provider,
        accountId: accountId ?? undefined,
        ...scopedUsageArgs(peerId),
      }, invoke);
      if (!cancelledRef.current && read === readSequence.current) setReading({ scope, snapshot: row ?? null });
    } catch (e) {
      if (DEV_BUILD) console.debug("Background usage snapshot read failed", e);
      if (!cancelledRef.current && read === readSequence.current) {
        if (peerId) setReading(null);
        setReadError({ scope, message: String(e) });
      }
    } finally {
      if (!cancelledRef.current && read === readSequence.current) setLoadingScope(null);
    }
  }, [provider, accountId, enabled, peerId, scope]);

  const refresh = useCallback(async () => {
    if (!enabled || peerId) return;
    setRefreshError(null);
    try {
      await invoke("usage_refresh", { provider, accountId: accountId ?? undefined, ...scopedUsageArgs(peerId) });
    } catch (e) {
      if (DEV_BUILD) console.debug("Background provider usage refresh failed", e);
      setRefreshError({ scope, message: String(e) });
    }
  }, [provider, accountId, enabled, peerId, scope]);

  useEffect(() => {
    cancelledRef.current = false;
    if (!enabled) return;
    let unlisten: (() => void) | null = null;
    let active = true;
    void subscribeUsageUpdates(peerId, provider, accountId, event => {
      if (active) void refetch(event);
    }, () => {
      ++readSequence.current;
      setReading(null);
      setLoadingScope(null);
    }).then((fn) => {
      if (!active) fn();
      else {
        unlisten = fn;
        void refetch();
      }
    });
    return () => {
      cancelledRef.current = true;
      active = false;
      ++readSequence.current;
      unlisten?.();
    };
  }, [provider, accountId, enabled, peerId, refetch]);

  useEffect(() => {
    if (!enabled || peerId || !snapshot || snapshot.parsed_ok === undefined) return;
    const due = nextResetAt(snapshot, Date.now());
    if (due == null) return;
    const delay = Math.min(Math.max(0, due - Date.now()) + 500, MAX_RESET_DELAY_MS);
    const t = setTimeout(() => {
      if (cancelledRef.current) return;
      const rolled = zeroExpiredResets(snapshot, Date.now());
      if (rolled !== snapshot) {
        setReading({ scope, snapshot: rolled });
        refresh();
      }
      setResetTick((v) => v + 1);
    }, delay);
    return () => clearTimeout(t);
  }, [snapshotKey, enabled, peerId, refresh, resetTick, scope]);

  const error = [readError, refreshError].find(entry => entry?.scope === scope)?.message ?? null;
  return { snapshot, loading: enabled && loadingScope === scope, error: enabled ? error : null, refetch, refresh };
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
  const { provider, accountId = null, sinceMs, limit = 200, enabled = true, refetchToken = 0 } = opts;
  const read = useUsageRead<UsageSnapshot[]>("usage_history", provider, accountId, enabled, sinceMs, limit, refetchToken);
  return { rows: read.value ?? [], loading: read.loading, error: read.error, refetch: read.refetch };
}

export type SessionProjection = import("../../gen/SessionProjection").SessionProjection;

export type DayActual = import("../../gen/DayActual").DayActual;

export type WeeklyProjection = import("../../gen/WeeklyProjection").WeeklyProjection;

export type ProjectionResponse = import("../../gen/ProjectionResponse").ProjectionResponse;

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
  const read = useUsageRead<ProjectionResponse>("usage_projection", provider, accountId, enabled);
  return { projection: read.value, loading: read.loading, error: read.error, refetch: read.refetch };
}

function useUsageRead<T>(
  command: "usage_history" | "usage_projection", provider: string, accountId: string | null,
  enabled: boolean, since?: number, limit?: number, refetchToken?: number,
) {
  const peerId = useUsageMachine();
  const scope = JSON.stringify([peerId, provider, accountId, since, limit]);
  const [reading, setReading] = useState<{ scope: string; value: T | null } | null>(null);
  const [loadingScope, setLoadingScope] = useState<string | null>(null);
  const [failure, setFailure] = useState<{ scope: string; error: string } | null>(null);
  const sequence = useRef(0);

  const refetch = useCallback(async (event?: UsageSnapshotEvent) => {
    if (!enabled) return;
    const read = ++sequence.current;
    setLoadingScope(scope);
    setFailure(null);
    try {
      const value = await readUsageOnEvent<T | null>(event, command, {
        provider, accountId: accountId ?? undefined, since, limit, ...scopedUsageArgs(peerId),
      }, invoke);
      if (read === sequence.current) setReading({ scope, value: value ?? null });
    } catch (error) {
      if (DEV_BUILD) console.debug("Background usage read failed", error);
      if (read !== sequence.current) return;
      if (peerId) setReading(null);
      setFailure({ scope, error: String(error) });
    } finally {
      if (read === sequence.current) setLoadingScope(null);
    }
  }, [accountId, command, enabled, peerId, limit, provider, scope, since]);

  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    void refetch();
    void subscribeUsageUpdates(peerId, provider, accountId, event => {
      if (active) void refetch(event);
    }, () => {
      ++sequence.current;
      setReading(null);
      setLoadingScope(null);
    }).then(stop => { if (active) unlisten = stop; else stop(); });
    return () => {
      active = false;
      ++sequence.current;
      unlisten?.();
    };
  }, [accountId, enabled, peerId, provider, refetch, refetchToken]);

  return {
    value: enabled && reading?.scope === scope ? reading.value : null,
    loading: enabled && loadingScope === scope,
    error: enabled && failure?.scope === scope ? failure.error : null,
    refetch,
  };
}
