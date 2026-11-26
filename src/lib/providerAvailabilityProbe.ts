import { invoke } from "./bridge";
import type { ProviderCliStatus } from "./providerCli";

export const PROVIDER_PROBE_TIMEOUT_MS = 15_000;

const providerProbes = new Map<string, Promise<ProviderCliStatus | null>>();

function probeKey(id: string, peerId?: string): string {
  return peerId ? `${peerId}\0${id}` : id;
}

export function probeProvider(
  id: string,
  peerId?: string,
  timeoutMs: number = PROVIDER_PROBE_TIMEOUT_MS,
): Promise<ProviderCliStatus | null> {
  const key = probeKey(id, peerId);
  const pending = providerProbes.get(key);
  if (pending) return pending;

  const args = peerId ? { providerId: id, peerId } : { providerId: id };
  let timer: ReturnType<typeof setTimeout> | undefined;
  const bounded = new Promise<ProviderCliStatus | null>((resolve) => {
    let settled = false;
    const finish = (status: ProviderCliStatus | null) => {
      if (settled) return;
      settled = true;
      if (timer !== undefined) clearTimeout(timer);
      resolve(status);
    };
    timer = setTimeout(() => finish(null), timeoutMs);
    void invoke<ProviderCliStatus>("provider_cli_status", args).then(
      (status) => finish(status ?? null),
      () => finish(null),
    );
  });

  const request = bounded.then((status) => {
    if (status === null && providerProbes.get(key) === request) {
      providerProbes.delete(key);
    }
    return status;
  });
  providerProbes.set(key, request);
  return request;
}

export function clearProviderProbes(peerId?: string): void {
  if (peerId === undefined) {
    providerProbes.clear();
    return;
  }
  const prefix = `${peerId}\0`;
  for (const key of providerProbes.keys()) {
    if (key.startsWith(prefix)) providerProbes.delete(key);
  }
}
