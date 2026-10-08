import { useSyncExternalStore } from "react";
import type { ChangesScope } from "../changesViewModel";

export type TabPanelView = "documents" | "changes";

export interface TabPanelState {
  documents: { detail: string | null };
  changes: { detail: string | null; scope: ChangesScope };
}

export type TabPanelAction =
  | { type: "open"; view: TabPanelView; id: string }
  | { type: "back"; view: TabPanelView }
  | { type: "scope"; scope: ChangesScope };

export const initialTabPanelState: TabPanelState = { documents: { detail: null }, changes: { detail: null, scope: "all" } };

export function tabPanelReducer(state: TabPanelState, action: TabPanelAction): TabPanelState {
  switch (action.type) {
    case "open":
      return state[action.view].detail === action.id ? state : { ...state, [action.view]: { ...state[action.view], detail: action.id } };
    case "back":
      return state[action.view].detail === null ? state : { ...state, [action.view]: { ...state[action.view], detail: null } };
    case "scope":
      return { ...state, changes: { detail: null, scope: action.scope } };
  }
}

/** The open detail when its item still exists; a vanished item falls back to the list. */
export function resolveDetail(detail: string | null, ids: readonly string[]): string | null {
  return detail !== null && ids.includes(detail) ? detail : null;
}

const states = new Map<string, TabPanelState>();
const listeners = new Set<() => void>();

export const tabPanelNavigationStore = {
  get(tabId: string): TabPanelState { return states.get(tabId) ?? initialTabPanelState; },
  dispatch(tabId: string, action: TabPanelAction) {
    const current = this.get(tabId);
    const next = tabPanelReducer(current, action);
    if (next === current) return;
    states.set(tabId, next);
    for (const listener of listeners) listener();
  },
  clear() { states.clear(); for (const listener of listeners) listener(); },
  subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
};

export function useTabPanelNavigation(tabId: string) {
  const state = useSyncExternalStore(tabPanelNavigationStore.subscribe, () => tabPanelNavigationStore.get(tabId), () => initialTabPanelState);
  return [state, (action: TabPanelAction) => tabPanelNavigationStore.dispatch(tabId, action)] as const;
}
