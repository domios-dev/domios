import type { ProviderDescriptor, TabBacking } from "./types";

export function usesPluginSessionModel(backing: TabBacking | null | undefined, descriptor: Pick<ProviderDescriptor, "agent_runtime"> | undefined): boolean {
  return backing?.kind === "chat_backend" && descriptor !== undefined && !descriptor.agent_runtime;
}

export function createSessionModelResource(
  fetch: (pluginId: string, sessionId: string) => Promise<string | null>,
  now: () => number = Date.now,
  ttl = 30_000,
) {
  const cache = new Map<string, { value: string | null; at: number }>();
  const inflight = new Map<string, Promise<string | null>>();
  const subscribers = new Set<() => void>();
  const key = (pluginId: string, sessionId: string) => JSON.stringify([pluginId, sessionId]);
  return {
    get(pluginId: string, sessionId: string): string | null {
      return cache.get(key(pluginId, sessionId))?.value ?? null;
    },
    subscribe(fn: () => void): () => void {
      subscribers.add(fn);
      return () => { subscribers.delete(fn); };
    },
    load(pluginId: string, sessionId: string): Promise<string | null> {
      const id = key(pluginId, sessionId);
      const previous = cache.get(id);
      if (previous && now() - previous.at < ttl) return Promise.resolve(previous.value);
      const active = inflight.get(id);
      if (active) return active;
      const request = fetch(pluginId, sessionId)
        .catch(() => previous?.value ?? null)
        .then(value => {
          cache.set(id, { value, at: now() });
          if (value !== (previous?.value ?? null)) for (const fn of subscribers) fn();
          return value;
        })
        .finally(() => { inflight.delete(id); });
      inflight.set(id, request);
      return request;
    },
  };
}
