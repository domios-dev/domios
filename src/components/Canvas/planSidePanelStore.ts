import { useSyncExternalStore } from "react";

/** What the app-level side panel shows: a task when taskId is set, the plan otherwise. */
export interface PlanSidePanelTarget {
  planId: string;
  taskId?: string;
  teamId?: string | null;
  canonicalProjectPath?: string;
}

export interface PlanSidePanelEntry {
  target: PlanSidePanelTarget;
  /** The element focused when the panel first opened; replacing the content keeps it. */
  opener: HTMLElement | null;
}

export const PLAN_SIDE_PANEL_ATTRIBUTE = "data-plan-side-panel";

let entry: PlanSidePanelEntry | null = null;
const listeners = new Set<() => void>();

function notify() {
  for (const listener of listeners) listener();
}

function focusedOutsidePanel(): HTMLElement | null {
  if (typeof document === "undefined") return null;
  const active = document.activeElement;
  if (!(active instanceof HTMLElement) || active === document.body) return null;
  return active.closest(`[${PLAN_SIDE_PANEL_ATTRIBUTE}]`) ? null : active;
}

export const planSidePanelStore = {
  open(target: PlanSidePanelTarget) {
    const outside = focusedOutsidePanel();
    entry = { target, opener: outside ?? entry?.opener ?? null };
    notify();
  },
  /** Returns the closed entry so a user dismissal can return focus to its opener. */
  close(): PlanSidePanelEntry | null {
    const closed = entry;
    if (!closed) return null;
    entry = null;
    notify();
    return closed;
  },
  get(): PlanSidePanelEntry | null {
    return entry;
  },
  subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
};

export function usePlanSidePanelEntry(): PlanSidePanelEntry | null {
  return useSyncExternalStore(planSidePanelStore.subscribe, planSidePanelStore.get, () => null);
}
