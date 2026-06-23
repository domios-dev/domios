import { useSyncExternalStore } from "react";

// Per-pane transient "Plan" view toggle (§7.1). Plan is a third pane mode peer to
// Chat|Terminal but, unlike them, never persists — it is a live view state only.
// Kept out of the binary PaneUiMode machinery on purpose (no storage ripple).
const active = new Set<string>();
const listeners = new Set<() => void>();

function notify() {
  for (const l of listeners) l();
}

export function setPanePlanMode(paneId: string, on: boolean): void {
  const had = active.has(paneId);
  if (on === had) return;
  if (on) active.add(paneId);
  else active.delete(paneId);
  notify();
}

export function isPanePlanMode(paneId: string): boolean {
  return active.has(paneId);
}

export function usePanePlanMode(paneId: string): boolean {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => active.has(paneId),
    () => false,
  );
}
