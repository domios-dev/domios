import { useSyncExternalStore } from "react";
import type { ProviderDescriptor } from "./types";

/**
 * Sync registry of agent provider descriptors keyed by id.
 *
 * Why a singleton outside React state: many small components
 * (AgentIcon in every sidebar tab/menu) need the descriptor to render the
 * brand mark. Routing every read through `useAgentProviders` would mean
 * each AgentIcon mounts its own listener and triggers its own initial
 * fetch. Instead, the existing `useAgentProviders` hook (used at higher
 * levels) populates this cache via `setAgentDescriptors`, and AgentIcon
 * reads sync via `useAgentDescriptor`. No new fetch storms, no flicker
 * after the first parent fetches once.
 */

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
