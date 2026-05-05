import { useSyncExternalStore } from "react";
import type { ProviderDescriptor } from "./types";

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

function subscribe(fn: () => void): () => void {
  subscribers.add(fn);
  return () => {
    subscribers.delete(fn);
  };
}

export function useAgentDescriptor(id: string | null | undefined): ProviderDescriptor | undefined {
  return useSyncExternalStore(
    subscribe,
    () => (id ? cache[id] : undefined),
    () => undefined,
  );
}
