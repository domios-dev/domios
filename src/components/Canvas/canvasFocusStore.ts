import { useSyncExternalStore } from "react";

export interface CanvasFocusTarget {
  planId?: string;
  orchestratorTabId?: string;
  /** Tab that opened focus — camera lands on its assigned task(s) when present. */
  focusTabId?: string;
}

export interface CanvasWorkspaceTarget {
  teamId?: string;
  managerTabId?: string | null;
  planId?: string;
  focusTabId?: string;
  nodeIds?: string[];
}

let open = false;
let target: CanvasFocusTarget | null = null;
let workspaceOpen = false;
let workspaceDismissed = false;
let workspaceTarget: CanvasWorkspaceTarget | null = null;
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
  openWorkspace(next?: CanvasWorkspaceTarget) {
    workspaceOpen = true;
    workspaceDismissed = false;
    workspaceTarget = next ?? null;
    notify();
  },
  closeWorkspace() {
    workspaceOpen = false;
    workspaceDismissed = true;
    workspaceTarget = null;
    notify();
  },
  toggleWorkspace(currentlyVisible: boolean) {
    if (currentlyVisible) {
      workspaceOpen = false;
      workspaceDismissed = true;
      workspaceTarget = null;
    } else {
      workspaceOpen = true;
      workspaceDismissed = false;
      workspaceTarget = null;
    }
    notify();
  },
  getWorkspaceTarget(): CanvasWorkspaceTarget | null {
    return workspaceTarget;
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

export function useCanvasWorkspaceTarget(): CanvasWorkspaceTarget | null {
  return useSyncExternalStore(
    canvasFocusStore.subscribe,
    () => canvasFocusStore.getWorkspaceTarget(),
    () => null,
  );
}

export function usePlanPulseVersion(planIds: string[]): number {
  return useSyncExternalStore(
    canvasFocusStore.subscribe,
    () => (canvasFocusStore.anyPulsing(planIds) ? 1 : 0) + planIds.length,
    () => 0,
  );
}
