import { useCallback, useSyncExternalStore } from "react";

export const SURFACE_ACTIVITY_EVENT = "surface-activity";
/** One droplet pass; matches --ct-duration-liquid-sweep. */
export const SURFACE_PULSE_MS = 560;

export interface SurfaceActivityPayload {
  tab_id: string;
  verb: string;
}

export interface SurfacePulse {
  nonce: number;
  verb: string;
}

export function parseSurfaceActivity(payload: unknown): SurfaceActivityPayload | null {
  if (typeof payload !== "object" || payload === null) return null;
  const { tab_id, verb } = payload as Record<string, unknown>;
  if (typeof tab_id !== "string" || !tab_id) return null;
  return { tab_id, verb: typeof verb === "string" ? verb : "" };
}

export interface SurfaceActivityStore {
  record(payload: unknown): void;
  pulse(tabId: string): SurfacePulse | null;
  subscribe(listener: () => void): () => void;
  reset(): void;
}

export function createSurfaceActivityStore(durationMs = SURFACE_PULSE_MS): SurfaceActivityStore {
  const pulses = new Map<string, SurfacePulse>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  const listeners = new Set<() => void>();
  let nonce = 0;
  const emit = () => {
    for (const listener of listeners) listener();
  };
  return {
    record(payload) {
      const activity = parseSurfaceActivity(payload);
      if (!activity) return;
      const { tab_id: tabId, verb } = activity;
      nonce += 1;
      pulses.set(tabId, { nonce, verb });
      const pending = timers.get(tabId);
      if (pending) clearTimeout(pending);
      timers.set(tabId, setTimeout(() => {
        timers.delete(tabId);
        pulses.delete(tabId);
        emit();
      }, durationMs));
      emit();
    },
    pulse: (tabId) => pulses.get(tabId) ?? null,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    reset() {
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
      pulses.clear();
      emit();
    },
  };
}

export const surfaceActivityStore = createSurfaceActivityStore();

export function recordSurfaceActivity(payload: unknown): void {
  surfaceActivityStore.record(payload);
}

/** The pass in flight for a surface tab, or null once it has landed. */
export function useSurfacePulse(surfaceTabId: string | null | undefined, store: SurfaceActivityStore = surfaceActivityStore): SurfacePulse | null {
  const read = useCallback(() => (surfaceTabId ? store.pulse(surfaceTabId) : null), [store, surfaceTabId]);
  return useSyncExternalStore(store.subscribe, read, read);
}
