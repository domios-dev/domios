import { useSyncExternalStore } from "react";

const focused = new Set<string>();
const listeners = new Set<() => void>();

function notify() {
  for (const l of listeners) l();
}

export const workerFocusStore = {
  isFocused(paneId: string): boolean {
    return focused.has(paneId);
  },
  focus(paneId: string) {
    if (focused.has(paneId)) return;
    focused.add(paneId);
    notify();
  },
  unfocus(paneId: string) {
    if (!focused.has(paneId)) return;
    focused.delete(paneId);
    notify();
  },
  snapshot(): ReadonlySet<string> {
    return focused;
  },
  subscribe(cb: () => void): () => void {
    listeners.add(cb);
    return () => {
      listeners.delete(cb);
    };
  },
};

export function isPaneVisible(
  pane: { hidden?: boolean; id: string } | null | undefined,
): boolean {
  if (!pane) return false;
  if (!pane.hidden) return true;
  return workerFocusStore.isFocused(pane.id);
}

export function useWorkerFocusVersion(): number {
  return useSyncExternalStore(
    workerFocusStore.subscribe,
    () => focused.size,
    () => 0,
  );
}
