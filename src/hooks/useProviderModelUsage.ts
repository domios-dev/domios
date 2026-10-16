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
): ProviderModelUsageState {
  const [usage, setUsage] = useState<ModelGroupUsage[]>(EMPTY_USAGE);
  const [refreshToken, setRefreshToken] = useState(0);

  useEffect(() => {
    if (!providerId || !enabled) return;
    let active = true;
    let stop: (() => void) | undefined;
    void listen<UsageSnapshotEvent>("usage-snapshot-updated", ({payload}) => {
      if (active && payload.provider === providerId) setRefreshToken(value => value + 1);
    }).then(unlisten => { if (active) stop = unlisten; else unlisten(); });
    return () => { active = false; stop?.(); };
  }, [providerId, enabled]);

  useEffect(() => {
    let active = true;
    if (!providerId || !enabled) {
      setUsage(EMPTY_USAGE);
      return;
    }
    void invoke<unknown>("provider_model_usage", { providerId })
      .then((result) => {
        if (active) setUsage(modelGroupUsageFromResponse(result));
      })
      .catch(() => {
        // Web mirror or a provider without a usage source: show none, never a guess.
        if (active) setUsage(EMPTY_USAGE);
      });
    return () => {
      active = false;
    };
  }, [providerId, enabled, refreshToken]);

  const refresh = useCallback(() => setRefreshToken((v) => v + 1), []);

  return { usage, refresh };
}

export function useProviderModelUsage(providerId: string | null): ModelGroupUsage[] {
  return useProviderModelUsageState(providerId).usage;
}
