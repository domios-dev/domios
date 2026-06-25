import { useSyncExternalStore } from "react";

export interface CanvasFocusTarget {
  planId?: string;
  orchestratorPaneId?: string;
  /** Pane that opened focus — camera lands on its assigned task(s) when present. */
  focusPaneId?: string;
}

let open = false;
let target: CanvasFocusTarget | null = null;
let workspaceOpen = false;
let workspaceDismissed = false;
const pulsing = new Set<string>();
const listeners = new Set<() => void>();

function notify() {
  for (const l of listeners) l();
}

export const canvasFocusStore = {
  openFocus(next: CanvasFocusTarget) {
    open = true;
    target = next;
    if (next.planId) pulsing.delete(next.planId);
    notify();
  },
  close() {
    if (!open) return;
    open = false;
    target = null;
    notify();
  },
  openWorkspace() {
    workspaceOpen = true;
    workspaceDismissed = false;
    notify();
  },
  closeWorkspace() {
    workspaceOpen = false;
    workspaceDismissed = true;
    notify();
  },
  toggleWorkspace(currentlyVisible: boolean) {
    if (currentlyVisible) {
      workspaceOpen = false;
      workspaceDismissed = true;
    } else {
      workspaceOpen = true;
      workspaceDismissed = false;
    }
    notify();
  },
  resetWorkspaceDismissed() {
    if (!workspaceDismissed) return;
    workspaceDismissed = false;
    notify();
  },
  isWorkspaceOpen(): boolean {
    return workspaceOpen;
  },
  isWorkspaceDismissed(): boolean {
    return workspaceDismissed;
  },
  isOpen(): boolean {
    return open;
  },
  getTarget(): CanvasFocusTarget | null {
    return target;
  },
  markPulse(planId: string) {
    if (pulsing.has(planId)) return;
    pulsing.add(planId);
    notify();
  },
  isPulsing(planId: string): boolean {
    return pulsing.has(planId);
  },
  anyPulsing(planIds: string[]): boolean {
    return planIds.some((id) => pulsing.has(id));
  },
  clearPulse(planId: string) {
    if (!pulsing.delete(planId)) return;
    notify();
  },
  subscribe(cb: () => void): () => void {
    listeners.add(cb);
    return () => listeners.delete(cb);
  },
};

export function useCanvasFocusOpen(): boolean {
  return useSyncExternalStore(canvasFocusStore.subscribe, () => canvasFocusStore.isOpen(), () => false);
}

export function useCanvasWorkspaceOpen(): boolean {
  return useSyncExternalStore(
    canvasFocusStore.subscribe,
    () => canvasFocusStore.isWorkspaceOpen(),
    () => false,
  );
}

export function useCanvasWorkspaceDismissed(): boolean {
  return useSyncExternalStore(
    canvasFocusStore.subscribe,
    () => canvasFocusStore.isWorkspaceDismissed(),
    () => false,
  );
}

export function useCanvasFocusTarget(): CanvasFocusTarget | null {
  return useSyncExternalStore(canvasFocusStore.subscribe, () => canvasFocusStore.getTarget(), () => null);
}

export function usePlanPulseVersion(planIds: string[]): number {
  return useSyncExternalStore(
    canvasFocusStore.subscribe,
    () => (canvasFocusStore.anyPulsing(planIds) ? 1 : 0) + planIds.length,
    () => 0,
  );
}
