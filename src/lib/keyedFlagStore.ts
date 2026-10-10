import { useCallback, useSyncExternalStore } from "react";

export interface KeyedFlagStore {
  get(key: string): boolean;
  set(key: string, on: boolean): void;
  subscribe(listener: () => void): () => void;
  reset(): void;
}

export function createKeyedFlagStore(): KeyedFlagStore {
  const on = new Set<string>();
  const listeners = new Set<() => void>();
  const emit = () => {
    for (const listener of listeners) listener();
  };
  return {
    get: (key) => on.has(key),
    set(key, value) {
      if (on.has(key) === value) return;
      if (value) on.add(key);
      else on.delete(key);
      emit();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    reset() {
      if (on.size === 0) return;
      on.clear();
      emit();
    },
  };
}

export function useKeyedFlag(store: KeyedFlagStore, key: string | null | undefined): boolean {
  const read = useCallback(() => (key ? store.get(key) : false), [store, key]);
  return useSyncExternalStore(store.subscribe, read, read);
}
