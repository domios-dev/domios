import { useSyncExternalStore } from "react";

const focused = new Set<string>();
const listeners = new Set<() => void>();

function notify() {
  for (const l of listeners) l();
}

export const workerFocusStore = {
  isFocused(tabId: string): boolean {
    return focused.has(tabId);
  },
  focus(tabId: string) {
    if (focused.has(tabId)) return;
    focused.add(tabId);
    notify();
  },
  unfocus(tabId: string) {
    if (!focused.has(tabId)) return;
    focused.delete(tabId);
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

export function isTabVisible(
  tab: { hidden?: boolean; id: string } | null | undefined,
): boolean {
  if (!tab) return false;
  if (!tab.hidden) return true;
  return workerFocusStore.isFocused(tab.id);
}

export function useWorkerFocusVersion(): number {
  return useSyncExternalStore(
    workerFocusStore.subscribe,
    () => focused.size,
    () => 0,
  );
}
