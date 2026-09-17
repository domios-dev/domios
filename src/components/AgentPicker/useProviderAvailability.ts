import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { ProviderDescriptor } from "../../lib/types";

export type ProviderAvailabilityState = "idle" | "checking" | "ready" | "error";

/**
 * Re-check drops each declared binary's memoized resolution, then re-reads the
 * descriptors so the picker shows the same field the API serves and the CLI
 * renders. The picker never probes binaries itself.
 */
export async function recheckProviderDescriptors(
  providers: ProviderDescriptor[],
): Promise<ProviderDescriptor[]> {
  const binaries = [
    ...new Set(providers.map((p) => p.cli_binary).filter((b): b is string => Boolean(b))),
  ];
  await Promise.all(
    binaries.map((binary) =>
      invoke<boolean>("check_cli_installed", { binary }).catch(() => false),
    ),
  );
  const refreshed = await invoke<unknown>("list_agent_providers");
  return Array.isArray(refreshed) ? (refreshed as ProviderDescriptor[]) : providers;
}

function sameAvailability(a: ProviderDescriptor[], b: ProviderDescriptor[]): boolean {
  return (
    a.length === b.length &&
    a.every((p, i) => p.id === b[i].id && p.availability === b[i].availability)
  );
}

export function useProviderAvailability(providers: ProviderDescriptor[]) {
  const [observed, setObserved] = useState<ProviderDescriptor[]>(providers);
  const [state, setState] = useState<ProviderAvailabilityState>("ready");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setObserved(providers);
  }, [providers]);

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
