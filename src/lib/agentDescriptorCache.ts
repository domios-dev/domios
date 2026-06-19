import type { ProviderDescriptor } from "./types";

// React-free store so pure helpers (chatUiMode, paneProvider) can read it under
// the tsx unit-test runner. The React hook lives in `useAgentDescriptor.ts`.
let cache: Record<string, ProviderDescriptor> = {};
const subscribers = new Set<() => void>();

export function setAgentDescriptors(list: ProviderDescriptor[]): void {
  const next: Record<string, ProviderDescriptor> = {};
  for (const p of list) next[p.id] = p;
  cache = next;
  for (const fn of subscribers) fn();
}

export function getAgentDescriptor(id: string | null | undefined): ProviderDescriptor | undefined {
  if (!id) return undefined;
  return cache[id];
}

export function subscribeAgentDescriptors(fn: () => void): () => void {
  subscribers.add(fn);
  return () => {
    subscribers.delete(fn);
  };
}
