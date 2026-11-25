import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../lib/bridge";
import type { ModelGroupUsage } from "../lib/types";
import { scopedUsageArgs, subscribeUsageUpdates, useUsageMachine } from "./usageMachine";

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
  const peerId = useUsageMachine();
  const scope = JSON.stringify([peerId, providerId, accountId]);
  const [reading, setReading] = useState<{ scope: string; usage: ModelGroupUsage[] } | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);
  const readSequence = useRef(0);
  const refresh = useCallback(() => setRefreshToken((v) => v + 1), []);

  useEffect(() => {
    if (!providerId || !enabled) return;
    let active = true;
    let stop: (() => void) | undefined;
    void subscribeUsageUpdates(peerId, providerId, accountId, () => {
      if (active) refresh();
    }, () => {
      ++readSequence.current;
      setReading(null);
    }).then(unlisten => { if (active) stop = unlisten; else unlisten(); });
    return () => { active = false; stop?.(); };
  }, [providerId, enabled, accountId, peerId, refresh]);

  useEffect(() => {
    if (!providerId || !enabled) return;
    const read = ++readSequence.current;
    const publish = (usage: ModelGroupUsage[]) => {
      if (read === readSequence.current) setReading({ scope, usage });
    };
    void invoke<unknown>("provider_model_usage", { providerId, accountId: accountId ?? undefined, ...scopedUsageArgs(peerId) })
      .then(result => publish(modelGroupUsageFromResponse(result)), () => publish(EMPTY_USAGE));
    return () => {
      ++readSequence.current;
    };
  }, [providerId, enabled, accountId, peerId, scope, refreshToken]);

  const usage = enabled && reading?.scope === scope ? reading.usage : EMPTY_USAGE;
  return { usage, refresh };
}

export function useProviderModelUsage(providerId: string | null): ModelGroupUsage[] {
  return useProviderModelUsageState(providerId).usage;
}
