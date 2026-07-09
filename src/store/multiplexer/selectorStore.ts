import { useContext, useMemo } from "react";
import { useSyncExternalStoreWithSelector } from "use-sync-external-store/shim/with-selector";
import type { ExtendedState } from "./tree";
import { MuxStateStoreContext } from "./context";

export type MuxStateStore = {
  subscribe: (listener: () => void) => () => void;
  getState: () => ExtendedState;
  notify: () => void;
};

export function createMuxStateStore(getState: () => ExtendedState): MuxStateStore {
  const listeners = new Set<() => void>();
  return {
    getState,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    notify() {
      for (const listener of listeners) listener();
    },
  };
}

export function useMuxStateStore(): MuxStateStore {
  const store = useContext(MuxStateStoreContext);
  if (!store) throw new Error("useMuxStateStore must be used within MultiplexerProvider");
  return store;
}

export function useMuxState(): ExtendedState {
  const store = useMuxStateStore();
  return useSyncExternalStoreWithSelector(
    store.subscribe,
    store.getState,
    store.getState,
    (s) => s,
    (a, b) => a === b,
  );
}

export function useMuxSelector<T>(
  selector: (state: ExtendedState) => T,
  isEqual?: (a: T, b: T) => boolean,
): T {
  const store = useMuxStateStore();
  const stableSelector = useMemo(() => selector, [selector]);
  const stableIsEqual = useMemo(() => isEqual ?? Object.is, [isEqual]);
  return useSyncExternalStoreWithSelector(
    store.subscribe,
    store.getState,
    store.getState,
    stableSelector,
    stableIsEqual,
  );
}
