import { useCallback, useSyncExternalStore } from "react";

export const SURFACE_ACTIVITY_EVENT = "surface-activity";
/** One droplet pass; matches --ct-duration-liquid-sweep. */
export const SURFACE_PULSE_MS = 560;
/** How long a visiting agent's name stays on the surface after its last verb. */
export const SURFACE_ACTOR_LINGER_MS = 4000;
const MAX_ACTOR = 48;

export interface SurfaceActivityPayload {
  tab_id: string;
  verb: string;
  /** The acting agent, present only when it is not the surface owner. */
  actor?: string;
}

export interface SurfacePulse {
  nonce: number;
  verb: string;
}

export function parseSurfaceActivity(payload: unknown): SurfaceActivityPayload | null {
  if (typeof payload !== "object" || payload === null) return null;
  const { tab_id, verb, actor } = payload as Record<string, unknown>;
  if (typeof tab_id !== "string" || !tab_id) return null;
  const parsed: SurfaceActivityPayload = { tab_id, verb: typeof verb === "string" ? verb : "" };
  const name = typeof actor === "string" ? actor.trim().slice(0, MAX_ACTOR) : "";
  if (name) parsed.actor = name;
  return parsed;
}

export interface SurfaceActivityStore {
  record(payload: unknown): void;
  pulse(tabId: string): SurfacePulse | null;
  actor(tabId: string): string | null;
  subscribe(listener: () => void): () => void;
  reset(): void;
}

export function createSurfaceActivityStore(durationMs = SURFACE_PULSE_MS, actorLingerMs = SURFACE_ACTOR_LINGER_MS): SurfaceActivityStore {
  const pulses = new Map<string, SurfacePulse>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  const actors = new Map<string, string>();
  const actorTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const dropActor = (tabId: string) => {
    const pending = actorTimers.get(tabId);
    if (pending) clearTimeout(pending);
    actorTimers.delete(tabId);
    actors.delete(tabId);
  };
  const listeners = new Set<() => void>();
  let nonce = 0;
  const emit = () => {
    for (const listener of listeners) listener();
  };
  return {
    record(payload) {
      const activity = parseSurfaceActivity(payload);
      if (!activity) return;
      const { tab_id: tabId, verb, actor } = activity;
      dropActor(tabId);
      if (actor) {
        actors.set(tabId, actor);
        actorTimers.set(tabId, setTimeout(() => {
          actorTimers.delete(tabId);
          actors.delete(tabId);
          emit();
        }, actorLingerMs));
      }
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
    actor: (tabId) => actors.get(tabId) ?? null,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    reset() {
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
      pulses.clear();
      for (const timer of actorTimers.values()) clearTimeout(timer);
      actorTimers.clear();
      actors.clear();
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

/** The non-owner agent acting on a surface tab right now, or null. */
export function useSurfaceActor(surfaceTabId: string | null | undefined, store: SurfaceActivityStore = surfaceActivityStore): string | null {
  const read = useCallback(() => (surfaceTabId ? store.actor(surfaceTabId) : null), [store, surfaceTabId]);
  return useSyncExternalStore(store.subscribe, read, read);
}
