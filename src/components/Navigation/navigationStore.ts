import { useSyncExternalStore } from "react";

export interface MemoryReturnState {
  panX: number;
  panY: number;
  scale: number;
  selectedNodeId: string | null;
  feedVisible: boolean;
  workerPanelVisible: boolean;
}

export interface NavReturnState {
  view: "memory-panel";
  memoryState: MemoryReturnState;
  label: string;
}

let current: NavReturnState | null = null;
const listeners = new Set<() => void>();

function notify() {
  for (const l of listeners) l();
}

export const navigationStore = {
  get(): NavReturnState | null {
    return current;
  },
  set(state: NavReturnState | null) {
    current = state;
    notify();
  },
  clear() {
    if (current === null) return;
    current = null;
    notify();
  },
  subscribe(cb: () => void): () => void {
    listeners.add(cb);
    return () => {
      listeners.delete(cb);
    };
  },
};

export function useNavigationReturn(): NavReturnState | null {
  return useSyncExternalStore(
    navigationStore.subscribe,
    navigationStore.get,
    () => null,
  );
}
