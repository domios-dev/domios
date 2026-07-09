import { useSyncExternalStore } from "react";
import { invoke } from "./bridge";
import type { PluginSnapshot } from "../components/Extensions/types";

const TTL_MS = 15_000;
const EMPTY_SNAPSHOT: PluginSnapshot = { plugins: [], channels: [], diff: [], marketplace: [] };

export interface PluginSnapshotStoreState {
  snapshot: PluginSnapshot;
  lastSuccessfulSnapshot: PluginSnapshot | null;
  error: string | null;
  loading: boolean;
  refreshing: boolean;
  hasLoaded: boolean;
  lastLoadedAt: number;
}

type Listener = () => void;

let snapshotState: PluginSnapshotStoreState = {
  snapshot: EMPTY_SNAPSHOT,
  lastSuccessfulSnapshot: null,
  error: null,
  loading: false,
  refreshing: false,
  hasLoaded: false,
  lastLoadedAt: 0,
};

let inFlight: Promise<PluginSnapshot> | null = null;
const listeners = new Set<Listener>();

function emit() {
  for (const listener of listeners) listener();
}

function setSnapshotState(next: Partial<PluginSnapshotStoreState>) {
  snapshotState = { ...snapshotState, ...next };
  emit();
}

function normalizeSnapshot(value: PluginSnapshot | null | undefined): PluginSnapshot {
  return {
    plugins: value?.plugins ?? [],
    channels: value?.channels ?? [],
    diff: value?.diff ?? [],
    marketplace: value?.marketplace ?? [],
  };
}

function isFresh(now: number) {
  return snapshotState.hasLoaded && now - snapshotState.lastLoadedAt < TTL_MS;
}

async function fetchSnapshot({
  force = false,
  background = false,
}: {
  force?: boolean;
  background?: boolean;
} = {}): Promise<PluginSnapshot> {
  const now = Date.now();
  if (!force && isFresh(now)) return snapshotState.snapshot;
  if (inFlight) return inFlight;

  const hasCachedRows = snapshotState.lastSuccessfulSnapshot !== null;
  setSnapshotState({
    loading: !hasCachedRows && !background,
    refreshing: hasCachedRows || background,
    error: background ? snapshotState.error : null,
  });

  inFlight = invoke<PluginSnapshot>("plugin_manager_snapshot")
    .then((snapshot) => {
      const normalized = normalizeSnapshot(snapshot);
      setSnapshotState({
        snapshot: normalized,
        lastSuccessfulSnapshot: normalized,
        error: null,
        loading: false,
        refreshing: false,
        hasLoaded: true,
        lastLoadedAt: Date.now(),
      });
      return normalized;
    })
    .catch((error: unknown) => {
      setSnapshotState({
        snapshot: snapshotState.lastSuccessfulSnapshot ?? EMPTY_SNAPSHOT,
        error: String(error),
        loading: false,
        refreshing: false,
        hasLoaded: snapshotState.hasLoaded || snapshotState.lastSuccessfulSnapshot !== null,
      });
      throw error;
    })
    .finally(() => {
      inFlight = null;
    });

  return inFlight;
}

export function usePluginSnapshotStore(): PluginSnapshotStoreState {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => snapshotState,
  );
}

export function ensurePluginSnapshot(options?: { force?: boolean; background?: boolean }) {
  return fetchSnapshot(options);
}

export async function runPluginSnapshotAction(action: Record<string, unknown>): Promise<PluginSnapshot> {
  const next = normalizeSnapshot(await invoke<PluginSnapshot>("plugin_manager_action", { action }));
  setSnapshotState({
    snapshot: next,
    lastSuccessfulSnapshot: next,
    error: null,
    loading: false,
    refreshing: false,
    hasLoaded: true,
    lastLoadedAt: Date.now(),
  });
  return next;
}

export function invalidatePluginSnapshotStore() {
  setSnapshotState({ lastLoadedAt: 0 });
}

export function __resetPluginSnapshotStoreForTest() {
  inFlight = null;
  snapshotState = {
    snapshot: EMPTY_SNAPSHOT,
    lastSuccessfulSnapshot: null,
    error: null,
    loading: false,
    refreshing: false,
    hasLoaded: false,
    lastLoadedAt: 0,
  };
  emit();
}
