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

  return { observed, state, error, refresh };
}
