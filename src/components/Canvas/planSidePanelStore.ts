import { useSyncExternalStore } from "react";
import type { ReturnToRoute } from "../../lib/returnTo";
import { readTabPanel, writeTabPanel } from "../../lib/tabPanelPersistence";

/** What the app-level side panel shows: a task when taskId is set, the plan otherwise. */
export interface PlanSidePanelTarget extends ReturnToRoute {
  planId: string;
  documentsTabId?: string;
  tabView?: "documents" | "changes";
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
const rememberedDocuments = new Map<string, PlanSidePanelTarget>();
const listeners = new Set<() => void>();

function validTarget(value: unknown): value is PlanSidePanelTarget {
  if (!value || typeof value !== "object") return false;
  const target = value as PlanSidePanelTarget;
  return typeof target.planId === "string" && typeof target.documentsTabId === "string"
    && (target.tabView === "documents" || target.tabView === "changes" || target.tabView === undefined);
}

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
    if (target.documentsTabId) {
      rememberedDocuments.set(target.documentsTabId, target);
      writeTabPanel("open", target.documentsTabId, target);
    }
    const outside = focusedOutsidePanel();
    entry = { target, opener: outside ?? entry?.opener ?? null };
    notify();
  },
  /** Returns the closed entry so a user dismissal can return focus to its opener. */
  close(): PlanSidePanelEntry | null {
    const closed = entry;
    if (!closed) return null;
    entry = null;
    if (closed.target.documentsTabId) {
      rememberedDocuments.delete(closed.target.documentsTabId);
      writeTabPanel("open", closed.target.documentsTabId, null);
    }
    notify();
    return closed;
  },
  activateTab(tabId: string) {
    if (entry?.target.documentsTabId === tabId) return;
    if (entry?.target.documentsTabId) { entry = null; notify(); }
    const remembered = rememberedDocuments.get(tabId) ?? readTabPanel("open", tabId, validTarget);
    if (remembered) this.open(remembered);
  },
  suspend() { entry = null; notify(); },
  resetDocuments() {
    for (const id of rememberedDocuments.keys()) writeTabPanel("open", id, null);
    rememberedDocuments.clear();
  },
  resetCache() { rememberedDocuments.clear(); },
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
