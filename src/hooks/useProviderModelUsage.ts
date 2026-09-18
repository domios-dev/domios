/**
 * Per-model-group usage for the picker, as declared by the provider plugin.
 * The backend command carries the provider's own answer, so a new provider
 * needs no change here. Fetches on provider change and after a catalog refresh.
 */
import { useEffect, useState } from "react";
import { invoke } from "../lib/bridge";
import type { ModelGroupUsage } from "../lib/types";

const EMPTY_USAGE: ModelGroupUsage[] = [];

export function modelGroupUsageFromResponse(result: unknown): ModelGroupUsage[] {
  return Array.isArray(result) ? (result as ModelGroupUsage[]) : EMPTY_USAGE;
}

export function useProviderModelUsage(providerId: string | null): ModelGroupUsage[] {
  const [usage, setUsage] = useState<ModelGroupUsage[]>(EMPTY_USAGE);

  useEffect(() => {
    let active = true;
    if (!providerId) {
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
  }, [providerId]);

  return usage;
}
