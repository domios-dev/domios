import { useSyncExternalStore } from "react";

/** Concierge tabs keyed by friendship; they live under their friend, never as standalone rows. */
let bindings: Record<string, string> = {};
let gatewayIds: ReadonlySet<string> = new Set();
const listeners = new Set<() => void>();

export function publishNeighborGateways(next: Record<string, string>) {
  const ids = new Set(Object.values(next));
  if (ids.size === gatewayIds.size && [...ids].every(id => gatewayIds.has(id))
    && Object.keys(next).length === Object.keys(bindings).length && Object.entries(next).every(([key, value]) => bindings[key] === value)) return;
  bindings = next;
  gatewayIds = ids;
  listeners.forEach(listener => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function useNeighborGatewayIds(): ReadonlySet<string> {
  return useSyncExternalStore(subscribe, () => gatewayIds);
}
