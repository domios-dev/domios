import { useCallback, useEffect, useState } from "react";
import { invoke, listen } from "../lib/bridge";
import type { ModelGroupUsage } from "../lib/types";
import type { UsageSnapshotEvent } from "../types/usage";

const EMPTY_USAGE: ModelGroupUsage[] = [];

export function modelGroupUsageFromResponse(result: unknown): ModelGroupUsage[] {
  return Array.isArray(result) ? (result as ModelGroupUsage[]) : EMPTY_USAGE;
}

export interface ProviderModelUsageState {
  usage: ModelGroupUsage[];
  /** Re-reads the provider's own usage source; the capsule's refresh action. */
  refresh: () => void;
}

export function useProviderModelUsageState(
  providerId: string | null,
  enabled = true,
  accountId: string | null = null,
): ProviderModelUsageState {
  const [reading, setReading] = useState<{ provider: string; account: string | null; usage: ModelGroupUsage[] } | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);

  useEffect(() => {
    if (!providerId || !enabled) return;
    let active = true;
    let stop: (() => void) | undefined;
    void listen<UsageSnapshotEvent>("usage-snapshot-updated", ({payload}) => {
      if (active && payload.provider === providerId && (payload.account_id ?? null) === accountId) setRefreshToken(value => value + 1);
    }).then(unlisten => { if (active) stop = unlisten; else unlisten(); });
    return () => { active = false; stop?.(); };
  }, [providerId, enabled, accountId]);

  useEffect(() => {
    let active = true;
    if (!providerId || !enabled) return;
    const publish = (usage: ModelGroupUsage[]) => {
      if (active) setReading({ provider: providerId, account: accountId, usage });
    };
    void invoke<unknown>("provider_model_usage", { providerId, accountId: accountId ?? undefined })
      .then(result => publish(modelGroupUsageFromResponse(result)), () => publish(EMPTY_USAGE));
    return () => {
      active = false;
    };
  }, [providerId, enabled, accountId, refreshToken]);

  const refresh = useCallback(() => setRefreshToken((v) => v + 1), []);

  const usage = enabled && reading?.provider === providerId && reading.account === accountId ? reading.usage : EMPTY_USAGE;
  return { usage, refresh };
}

export function useProviderModelUsage(providerId: string | null): ModelGroupUsage[] {
  return useProviderModelUsageState(providerId).usage;
}
