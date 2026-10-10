import type { ProviderDescriptor } from "./types";

// the tsx unit-test runner. The React hook lives in `useAgentDescriptor.ts`.
let cache: Record<string, ProviderDescriptor> = {};
// (useAgentDescriptors) doesn't see a fresh array every render.
let ordered: ProviderDescriptor[] = [];
let refreshing = false;
let catalogueVersions: Record<string, number> = {};
const subscribers = new Set<() => void>();

export function setAgentDescriptors(list: ProviderDescriptor[], loading = false): void {
  refreshing = loading;
  const next: Record<string, ProviderDescriptor> = {};
  const nextOrdered: ProviderDescriptor[] = [];
  const nextVersions = { ...catalogueVersions };
  for (const provider of list) {
    const incomingVersion = provider.catalogue_version ?? 0;
    const currentVersion = nextVersions[provider.id] ?? 0;
    if (incomingVersion < currentVersion) {
      const current = cache[provider.id];
      if (current) {
        next[provider.id] = current;
        nextOrdered.push(current);
      }
      continue;
    }
    next[provider.id] = provider;
    nextOrdered.push(provider);
    nextVersions[provider.id] = incomingVersion;
  }
  cache = next;
  ordered = nextOrdered;
  catalogueVersions = nextVersions;
  for (const fn of subscribers) fn();
}

export function acceptModelCatalogueChange(change: { provider: string; version: number }): boolean {
  const current = catalogueVersions[change.provider] ?? 0;
  if (change.version <= current) return false;
  catalogueVersions = { ...catalogueVersions, [change.provider]: change.version };
  return true;
}

export function getAgentDescriptor(id: string | null | undefined): ProviderDescriptor | undefined {
  if (!id) return undefined;
  return cache[id];
}

export function agentDescriptorsRefreshing(): boolean {
  return refreshing;
}

export function listAgentDescriptors(): ProviderDescriptor[] {
  return ordered;
}

export function defaultSpawnProviderId(): string {
  for (const d of ordered) {
    if (d.transport === "pty") return d.id;
  }
  return "claude";
}

export function subscribeAgentDescriptors(fn: () => void): () => void {
  subscribers.add(fn);
  return () => {
    subscribers.delete(fn);
  };
}
