import { useSyncExternalStore } from "react";

let enabled = false;
const listeners = new Set<() => void>();

export function applyCloudAvailability(value: unknown): void {
  const next = value === true;
  if (next === enabled) return;
  enabled = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function useCloudMachinesEnabled(): boolean {
  return useSyncExternalStore(subscribe, () => enabled, () => false);
}
