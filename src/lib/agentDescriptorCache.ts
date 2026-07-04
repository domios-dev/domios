import type { ProviderDescriptor } from "./types";

// the tsx unit-test runner. The React hook lives in `useAgentDescriptor.ts`.
let cache: Record<string, ProviderDescriptor> = {};
// (useAgentDescriptors) doesn't see a fresh array every render.
let ordered: ProviderDescriptor[] = [];
const subscribers = new Set<() => void>();

export function setAgentDescriptors(list: ProviderDescriptor[]): void {
  const next: Record<string, ProviderDescriptor> = {};
  for (const p of list) next[p.id] = p;
  cache = next;
  ordered = list.slice();
  for (const fn of subscribers) fn();
}

export function getAgentDescriptor(id: string | null | undefined): ProviderDescriptor | undefined {
  if (!id) return undefined;
  return cache[id];
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
