/**
 * Per-model-group usage for the picker and the usage capsule, as declared by
 * the provider plugin. The backend command carries the provider's own answer,
 * so a new provider needs no change here. Fetches on provider change and after
 * an explicit refresh.
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "../lib/bridge";
import type { ModelGroupUsage } from "../lib/types";

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
