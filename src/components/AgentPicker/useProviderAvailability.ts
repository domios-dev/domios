import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { ProviderDescriptor } from "../../lib/types";
import { providerRequiresCli } from "./preselectProvider";

export type ProviderAvailabilityState = "idle" | "checking" | "ready" | "error";

export async function probeProviderAvailability(
  providers: ProviderDescriptor[],
): Promise<Record<string, boolean>> {
  const binaries = [
    ...new Set(
      providers
        .map((p) => p.cli_binary)
        .filter((b): b is string => Boolean(b)),
    ),
  ];
  const entries = await Promise.all(
    binaries.map(async (binary) => {
      try {
        const installed = await invoke<boolean>("check_cli_installed", { binary });
        return [binary, installed] as const;
      } catch {
        return [binary, false] as const;
      }
    }),
  );
  return Object.fromEntries(entries);
}

export function useProviderAvailability(providers: ProviderDescriptor[]) {
  const [installedByBinary, setInstalledByBinary] = useState<Record<string, boolean>>({});
  const [state, setState] = useState<ProviderAvailabilityState>("idle");
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (providers.length === 0) {
      setInstalledByBinary({});
      setState("ready");
      return;
    }
    const needsProbe = providers.some(providerRequiresCli);
    if (!needsProbe) {
      setInstalledByBinary({});
      setState("ready");
      return;
    }
    setState("checking");
    setError(null);
    try {
      const next = await probeProviderAvailability(providers);
      setInstalledByBinary(next);
      setState("ready");
    } catch (e) {
      setState("error");
      setError(typeof e === "string" ? e : "Availability check failed");
    }
  }, [providers]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { installedByBinary, state, error, refresh };
}
