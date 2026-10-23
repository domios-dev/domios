import { useEffect, useRef } from "react";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { planSidePanelStore, usePlanSidePanelEntry } from "./planSidePanelStore";

/** The workspace and tab a drawer was opened from; any change of either dismisses it. */
export function selectDrawerContextKey(state: ExtendedState): string {
  const workspace = state.workspaces.find((candidate) => candidate.id === state.active_workspace_id);
  return `${state.active_workspace_id ?? ""}:${workspace?.active_tab_id ?? ""}`;
}

export interface DrawerBinding {
  bound: string | null;
  close: boolean;
}

/** `bound` is the context key the open drawer was bound to; `open` is whether a context-bound drawer is showing. */
export function reconcileDrawerBinding(bound: string | null, current: string, open: boolean): DrawerBinding {
  if (!open) return { bound: null, close: false };
  if (bound === null) return { bound: current, close: false };
  return bound === current ? { bound, close: false } : { bound: null, close: true };
}

/** Tab-bound drawers (Documents, Changes) follow their own tab through activateTab; plan and task drawers use this. */
export function usePlanSidePanelContextBinding(contextKey: string): void {
  const entry = usePlanSidePanelEntry();
  const open = entry !== null && !entry.target.documentsTabId;
  const bound = useRef<string | null>(null);
  useEffect(() => {
    const next = reconcileDrawerBinding(bound.current, contextKey, open);
    bound.current = next.bound;
    if (next.close) planSidePanelStore.close();
  }, [contextKey, open]);
}
