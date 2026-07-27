import type { Dispatch } from "react";
import type { MultiplexerState } from "../../lib/types";
import type { ExtendedState } from "./tree";
import type { Action } from "./reducer";

declare global {
  interface Window {
    __muxPerfChurnTab?: (tabId: string) => void;
    __renderCounts?: Record<string, number>;
  }
}

export function mountMuxPerfTestHook(
  enabled: boolean,
  getState: () => ExtendedState,
  dispatch: Dispatch<Action>,
): () => void {
  if (!enabled || typeof window === "undefined") return () => {};

  window.__muxPerfChurnTab = (tabId: string) => {
    const current = getState();
    const tab = current.tabs[tabId];
    if (!tab) return;
    const prevMs = tab.agent_state?.elapsed_ms ?? 0;
    const agent_state = {
      ...(tab.agent_state ?? { state: "working" as const }),
      elapsed_ms: prevMs + 50,
      status_line: `Perf churn ${prevMs + 50}ms`,
      phase: "work",
    };
    const nextGen =
      (typeof current.generation === "number" ? current.generation : 0) + 1;
    const snapshot: MultiplexerState = {
      workspaces: current.workspaces,
      tabs: {
        ...current.tabs,
        [tabId]: { ...tab, agent_state },
      },
      active_workspace_id: current.active_workspace_id,
      focused_tab_id: current.focused_tab_id,
      recently_closed: current.recently_closed,
      generation: nextGen,
      tab_statuses: current.tabStatuses,
    };
    dispatch({ type: "SET_STATE", state: snapshot });
  };

  return () => {
    delete window.__muxPerfChurnTab;
  };
}
