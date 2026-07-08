import type { Dispatch } from "react";
import type { MultiplexerState } from "../../lib/types";
import type { ExtendedState } from "./tree";
import type { Action } from "./reducer";

declare global {
  interface Window {
    __muxPerfChurnPane?: (paneId: string) => void;
    __renderCounts?: Record<string, number>;
  }
}

export function mountMuxPerfTestHook(
  enabled: boolean,
  getState: () => ExtendedState,
  dispatch: Dispatch<Action>,
): () => void {
  if (!enabled || typeof window === "undefined") return () => {};

  window.__muxPerfChurnPane = (paneId: string) => {
    const current = getState();
    const pane = current.panes[paneId];
    if (!pane) return;
    const prevMs = pane.agent_state?.elapsed_ms ?? 0;
    const agent_state = {
      ...(pane.agent_state ?? { state: "working" as const }),
      elapsed_ms: prevMs + 50,
      status_line: `Perf churn ${prevMs + 50}ms`,
      phase: "work",
    };
    const nextGen =
      (typeof current.generation === "number" ? current.generation : 0) + 1;
    const snapshot: MultiplexerState = {
      workspaces: current.workspaces,
      panes: {
        ...current.panes,
        [paneId]: { ...pane, agent_state },
      },
      active_workspace_id: current.active_workspace_id,
      focused_pane_id: current.focused_pane_id,
      recently_closed: current.recently_closed,
      generation: nextGen,
      pane_statuses: current.paneStatuses,
    };
    dispatch({ type: "SET_STATE", state: snapshot });
  };

  return () => {
    delete window.__muxPerfChurnPane;
  };
}
