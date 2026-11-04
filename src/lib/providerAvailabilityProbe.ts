import { invoke } from "./bridge";
import type { ProviderCliStatus } from "./providerCli";

const providerProbes = new Map<string, Promise<ProviderCliStatus | null>>();
export function probeProvider(id: string): Promise<ProviderCliStatus | null> {
  const pending = providerProbes.get(id);
  if (pending) return pending;
  const request = invoke<ProviderCliStatus>("provider_cli_status", { providerId: id })
    .catch(() => null);
  providerProbes.set(id, request);
  return request;
}

export function clearProviderProbes(): void {
  providerProbes.clear();
}
