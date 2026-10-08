import { useSyncExternalStore } from "react";
import { readTabPanel, writeTabPanel } from "../../../lib/tabPanelPersistence";
import type { ChangesScope } from "../changesViewModel";

export type ChangeDetailView = "diff" | "file";

export type TabPanelView = "documents" | "changes";

export interface TabPanelState {
  documents: { detail: string | null };
  changes: { detail: string | null; scope: ChangesScope; view: ChangeDetailView };
}

export type TabPanelAction =
  | { type: "open"; view: TabPanelView; id: string }
  | { type: "back"; view: TabPanelView }
  | { type: "change-view"; view: ChangeDetailView }
  | { type: "scope"; scope: ChangesScope };

export const initialTabPanelState: TabPanelState = { documents: { detail: null }, changes: { detail: null, scope: "all", view: "diff" } };

export function tabPanelReducer(state: TabPanelState, action: TabPanelAction): TabPanelState {
  switch (action.type) {
    case "open":
      return state[action.view].detail === action.id ? state : { ...state, [action.view]: { ...state[action.view], detail: action.id } };
    case "back":
      return state[action.view].detail === null ? state : { ...state, [action.view]: { ...state[action.view], detail: null } };
    case "change-view":
      return state.changes.view === action.view ? state : { ...state, changes: { ...state.changes, view: action.view } };
    case "scope":
      return { ...state, changes: { ...state.changes, detail: null, scope: action.scope } };
  }
}

/** The open detail when its item still exists; a vanished item falls back to the list. */
export function resolveDetail(detail: string | null, ids: readonly string[]): string | null {
  return detail !== null && ids.includes(detail) ? detail : null;
}

const states = new Map<string, TabPanelState>();
const listeners = new Set<() => void>();

function validState(value: unknown): value is TabPanelState {
  if (!value || typeof value !== "object") return false;
  const state = value as TabPanelState;
  const detail = (item: unknown) => item === null || typeof item === "string";
  return !!state.documents && !!state.changes && detail(state.documents.detail)
    && detail(state.changes.detail) && (state.changes.view === "diff" || state.changes.view === "file") && (state.changes.scope === "all"
      || (typeof state.changes.scope === "object" && state.changes.scope !== null
        && typeof state.changes.scope.turnId === "string"));
}

export const tabPanelNavigationStore = {
  get(tabId: string): TabPanelState {
    const cached = states.get(tabId);
    if (cached) return cached;
    const saved = readTabPanel("navigation", tabId, validState) ?? initialTabPanelState;
    states.set(tabId, saved);
    return saved;
  },
  dispatch(tabId: string, action: TabPanelAction) {
    const current = this.get(tabId);
    const next = tabPanelReducer(current, action);
    if (next === current) return;
    states.set(tabId, next);
    writeTabPanel("navigation", tabId, next);
    for (const listener of listeners) listener();
  },
  clear() {
    for (const id of states.keys()) writeTabPanel("navigation", id, null);
    this.resetCache();
  },
  resetCache() { states.clear(); for (const listener of listeners) listener(); },
  subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
};

export function useTabPanelNavigation(tabId: string) {
  const state = useSyncExternalStore(tabPanelNavigationStore.subscribe, () => tabPanelNavigationStore.get(tabId), () => initialTabPanelState);
  return [state, (action: TabPanelAction) => tabPanelNavigationStore.dispatch(tabId, action)] as const;
}
