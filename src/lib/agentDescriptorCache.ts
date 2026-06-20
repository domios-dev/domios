import type { ProviderDescriptor } from "./types";

// React-free store so pure helpers (chatUiMode, paneProvider) can read it under
// the tsx unit-test runner. The React hook lives in `useAgentDescriptor.ts`.
let cache: Record<string, ProviderDescriptor> = {};
// Registry-order snapshot; stable identity between updates so useSyncExternalStore
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

// Capability-driven default for "spawn an agent" affordances (new-tab button,
// command palette, memory worker dialog) and as a display fallback for records
// with no recorded provider. Picks the first registered PTY agent by capability,
// never a hardcoded vendor id. "claude" is only a last resort while the registry
// is still loading (cache empty) — the resolved default once it loads.
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
