import { useCallback, useEffect, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import {
  applyAvailabilityChange,
  PROVIDER_AVAILABILITY_CHANGED,
  type ProviderAvailabilityChange,
} from "../../lib/providerAvailability";
import type { ProviderCliStatus } from "../../lib/providerCli";
import type { ProviderDescriptor } from "../../lib/types";

export type ProviderAvailabilityState = "idle" | "checking" | "ready" | "error";

export async function recheckProviderDescriptors(
  providers: ProviderDescriptor[],
): Promise<ProviderDescriptor[]> {
  await Promise.all(providers.map((provider) =>
    invoke<ProviderCliStatus>("provider_cli_status", { providerId: provider.id }),
  ));
  const refreshed = await invoke<unknown>("list_agent_providers");
  return Array.isArray(refreshed) ? (refreshed as ProviderDescriptor[]) : providers;
}

function sameAvailability(a: ProviderDescriptor[], b: ProviderDescriptor[]): boolean {
  return (
    a.length === b.length &&
    a.every((p, i) => p.id === b[i].id && p.availability === b[i].availability)
  );
}

function availabilitySignature(providers: ProviderDescriptor[]): string {
  return JSON.stringify(providers.map((p) => [p.id, p.availability, p.display_name, p.install_command]));
}

export function useProviderAvailability(providers: ProviderDescriptor[]) {
  const [observed, setObserved] = useState<ProviderDescriptor[]>(providers);
  const [state, setState] = useState<ProviderAvailabilityState>("ready");
  const [error, setError] = useState<string | null>(null);

  // Parents rebuild the list every render; only a changed answer may replace a fresher probe.
  const signature = availabilitySignature(providers);
  const [adopted, setAdopted] = useState(signature);
  if (adopted !== signature) {
    setAdopted(signature);
    setObserved(providers);
  }

  useEffect(() => {
    let active = true;
    let stop: (() => void) | undefined;
    void listen<ProviderAvailabilityChange>(PROVIDER_AVAILABILITY_CHANGED, ({ payload }) => {
      if (active) setObserved((current) => applyAvailabilityChange(current, payload));
    }).then((unlisten) => { if (active) stop = unlisten; else unlisten(); }).catch(() => {});
    return () => { active = false; stop?.(); };
  }, []);

  const refresh = useCallback(async () => {
    setState("checking");
    setError(null);
    try {
      const ids = new Set(providers.map((p) => p.id));
      const refreshed = await recheckProviderDescriptors(providers);
      setObserved(refreshed.filter((p) => ids.has(p.id)));
      setState("ready");
    } catch (e) {
      setState("error");
      setError(typeof e === "string" ? e : "Availability check failed");
    }
  }, [providers]);

  /**
   * A background probe with no visible state: it never enters `checking` and
   * only replaces the list when an answer actually changed, so a watcher can
   * run without the UI blinking on every pass.
   */
  const silentRefresh = useCallback(async () => {
    try {
      const ids = new Set(providers.map((p) => p.id));
      const refreshed = (await recheckProviderDescriptors(providers)).filter((p) => ids.has(p.id));
      setObserved((cur) => (sameAvailability(cur, refreshed) ? cur : refreshed));
    } catch {
      /* a failed probe keeps the last good answer rather than blanking it */
    }
  }, [providers]);

  return { observed, state, error, refresh, silentRefresh };
}
