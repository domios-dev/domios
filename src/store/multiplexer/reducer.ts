import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Workspace,
  Tab,
  Pane,
  Orientation,
  Notification,
  PaneStatus,
} from "../../lib/types";
import {
  collectPaneIds,
  highestTitleLevel,
  removeLeaf,
  shortenTitle,
  splitLeaf,
  updateRatio,
  type ExtendedState,
} from "./tree";

export type Action =
  | { type: "SET_STATE"; state: MultiplexerState }
  | { type: "ADD_WORKSPACE"; workspace: Workspace; pane: Pane }
  | { type: "SET_ACTIVE_WORKSPACE"; id: string }
  | { type: "CLOSE_WORKSPACE"; id: string }
  | { type: "RENAME_WORKSPACE"; id: string; name: string }
  | { type: "MOVE_TAB"; tabId: string; fromWorkspaceId: string; toWorkspaceId: string }
  | { type: "ADD_TAB"; workspaceId: string; tab: Tab; pane: Pane }
  | { type: "CLOSE_TAB"; workspaceId: string; tabId: string; removedPaneIds: string[] }
  | { type: "SET_ACTIVE_TAB"; workspaceId: string; tabId: string }
  | {
      type: "SPLIT_PANE";
      workspaceId: string;
      tabId: string;
      targetPaneId: string;
      newPane: Pane;
      orientation: Orientation;
    }
  | {
      type: "CLOSE_PANE";
      workspaceId: string;
      tabId: string;
      paneId: string;
    }
  | { type: "SET_FOCUSED_PANE"; paneId: string }
  | { type: "SET_PANE_PTY"; paneId: string; ptyId: string }
  | { type: "UPDATE_SPLIT_RATIO"; workspaceId: string; tabId: string; path: number[]; ratio: number }
  | { type: "ADD_NOTIFICATION"; notification: Notification }
  | { type: "MARK_READ"; id: string }
  | { type: "MARK_ALL_READ" }
  | { type: "CLEAR_NOTIFICATIONS" }
  | { type: "DISMISS_TOAST"; id: string }
  | { type: "SET_PANE_TITLE"; paneId: string; title: string; level?: TitleLevel }
  | { type: "SET_AGENT_TYPE"; paneId: string; agentType: string | null }
  | { type: "UPDATE_PANE_STATUS"; paneId: string; status: PaneStatus };

export function reducer(state: ExtendedState, action: Action): ExtendedState {
  switch (action.type) {
    case "SET_STATE": {
      // Preserve frontend-authoritative split ratios when merging backend state.
      // Backend doesn't track ratio changes from divider dragging, so we must
      // keep ratios from the existing local state and only update structural
      // changes (new/removed panes, tabs, workspaces).
      const mergedWorkspaces = action.state.workspaces.map((bws) => {
        const existingWs = state.workspaces.find((w) => w.id === bws.id);
        if (!existingWs) return bws;
        return {
          ...bws,
          tabs: bws.tabs.map((bTab) => {
            const existingTab = existingWs.tabs.find((t) => t.id === bTab.id);
            if (!existingTab) return bTab;
            const existingPaneIds = collectPaneIds(existingTab.split_tree).sort().join(",");
            const backendPaneIds = collectPaneIds(bTab.split_tree).sort().join(",");
            if (existingPaneIds === backendPaneIds) {
              return { ...bTab, split_tree: existingTab.split_tree };
            }
            return bTab;
          }),
        };
      });
      const mergedPanes = { ...action.state.panes };
      for (const [id, backendPane] of Object.entries(mergedPanes)) {
        const existing = state.panes[id];
        const agentType = existing?.agent_type || backendPane.agent_type;
        const backendLevel = backendPane.title_level || TitleLevel.Default;
        const frontendLevel = existing?.title_level || TitleLevel.Default;
        const highestLevel = highestTitleLevel(backendLevel, frontendLevel);
        const title = highestLevel === frontendLevel && existing ? existing.title : backendPane.title;
        mergedPanes[id] = {
          ...backendPane,
          title_level: highestLevel as TitleLevel,
          title,
          ...(agentType ? { agent_type: agentType } : {}),
        };
      }
      const newToasts: Notification[] = [];
      for (const [id, pane] of Object.entries(mergedPanes)) {
        const oldPane = state.panes[id];
        if (pane.pending_prompt && !oldPane?.pending_prompt) {
          newToasts.push({
            id: `prompt-${id}-${Date.now()}`,
            pane_id: id,
            workspace_id: null,
            title: pane.title || "Agent",
            body: "Needs your attention",
            level: "Warning" as const,
            read: false,
            sticky: false,
            timestamp: Date.now(),
          });
        }
      }
      // `focused_pane_id` is intentionally omitted from the spread — focus
      // is propagated via the dedicated `pane-focused` event from backend, NOT
      // via blind merging of every state snapshot. Otherwise any background
      // emit (PTY chunk, worker spawn) would steal the user's selection.
      return {
        ...state,
        workspaces: mergedWorkspaces,
        panes: mergedPanes,
        active_workspace_id: action.state.active_workspace_id,
        toasts: [...state.toasts, ...newToasts],
      };
    }

    case "ADD_WORKSPACE": {
      return {
        ...state,
        workspaces: [...state.workspaces, action.workspace],
        panes: { ...state.panes, [action.pane.id]: action.pane },
        active_workspace_id: action.workspace.id,
      };
    }

    case "SET_ACTIVE_WORKSPACE":
      return { ...state, active_workspace_id: action.id };

    case "CLOSE_WORKSPACE": {
      const ws = state.workspaces.find((w) => w.id === action.id);
      const removedPaneIds = ws
        ? ws.tabs.flatMap((t) => collectPaneIds(t.split_tree))
        : [];
      const newPanes = { ...state.panes };
      const newStatuses = { ...state.paneStatuses };
      for (const pid of removedPaneIds) {
        delete newPanes[pid];
        delete newStatuses[pid];
      }
      const remaining = state.workspaces.filter((w) => w.id !== action.id);
      // focused_pane_id intentionally untouched — backend's close_workspace
      // emits `pane-focused` with the General fallback target, which the
      // listener applies separately. If we nulled here we'd flicker.
      return {
        ...state,
        workspaces: remaining,
        panes: newPanes,
        paneStatuses: newStatuses,
        active_workspace_id:
          state.active_workspace_id === action.id
            ? (remaining[0]?.id ?? null)
            : state.active_workspace_id,
      };
    }

    case "RENAME_WORKSPACE": {
      return {
        ...state,
        workspaces: state.workspaces.map((ws) =>
          ws.id === action.id ? { ...ws, name: action.name } : ws,
        ),
      };
    }

    case "MOVE_TAB": {
      const fromWs = state.workspaces.find((w) => w.id === action.fromWorkspaceId);
      const tab = fromWs?.tabs.find((t) => t.id === action.tabId);
      if (!fromWs || !tab || fromWs.tabs.length <= 1) return state;

      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (ws.id === action.fromWorkspaceId) {
            const newTabs = ws.tabs.filter((t) => t.id !== action.tabId);
            return {
              ...ws,
              tabs: newTabs,
              active_tab_id: ws.active_tab_id === action.tabId
                ? newTabs[newTabs.length - 1]?.id ?? null
                : ws.active_tab_id,
            };
          }
          if (ws.id === action.toWorkspaceId) {
            return {
              ...ws,
              tabs: [...ws.tabs, tab],
              active_tab_id: action.tabId,
            };
          }
          return ws;
        }),
        active_workspace_id: action.toWorkspaceId,
      };
    }

    case "ADD_TAB": {
      // No auto-focus on tab creation. The createTab caller decides whether to
      // focus the new tab (user-initiated UI clicks do, background spawns don't).
      return {
        ...state,
        workspaces: state.workspaces.map((ws) =>
          ws.id === action.workspaceId
            ? { ...ws, tabs: [...ws.tabs, action.tab], active_tab_id: action.tab.id }
            : ws,
        ),
        panes: { ...state.panes, [action.pane.id]: action.pane },
      };
    }

    case "CLOSE_TAB": {
      const newPanes = { ...state.panes };
      const newStatuses = { ...state.paneStatuses };
      for (const pid of action.removedPaneIds) {
        delete newPanes[pid];
        delete newStatuses[pid];
      }
      // focused_pane_id intentionally untouched — see CLOSE_WORKSPACE comment.
      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (ws.id !== action.workspaceId) return ws;
          const remaining = ws.tabs.filter((t) => t.id !== action.tabId);
          return {
            ...ws,
            tabs: remaining,
            active_tab_id:
              ws.active_tab_id === action.tabId
                ? (remaining[remaining.length - 1]?.id ?? null)
                : ws.active_tab_id,
          };
        }),
        panes: newPanes,
        paneStatuses: newStatuses,
      };
    }

    case "SET_ACTIVE_TAB":
      return {
        ...state,
        workspaces: state.workspaces.map((ws) =>
          ws.id === action.workspaceId
            ? { ...ws, active_tab_id: action.tabId }
            : ws,
        ),
      };

    case "SPLIT_PANE": {
      // Split UX is removed in the current build, but the reducer case stays
      // as a defense — focus is no longer auto-moved here either.
      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (ws.id !== action.workspaceId) return ws;
          return {
            ...ws,
            tabs: ws.tabs.map((tab) => {
              if (tab.id !== action.tabId) return tab;
              return {
                ...tab,
                split_tree: splitLeaf(
                  tab.split_tree,
                  action.targetPaneId,
                  action.newPane.id,
                  action.orientation,
                ),
              };
            }),
          };
        }),
        panes: { ...state.panes, [action.newPane.id]: action.newPane },
      };
    }

    case "CLOSE_PANE": {
      const newPanes = { ...state.panes };
      delete newPanes[action.paneId];
      const newStatuses = { ...state.paneStatuses };
      delete newStatuses[action.paneId];
      // focused_pane_id intentionally untouched — see CLOSE_WORKSPACE comment.
      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (ws.id !== action.workspaceId) return ws;
          return {
            ...ws,
            tabs: ws.tabs.map((tab) => {
              if (tab.id !== action.tabId) return tab;
              const newTree = removeLeaf(tab.split_tree, action.paneId);
              return {
                ...tab,
                split_tree: newTree ?? tab.split_tree,
              };
            }),
          };
        }),
        panes: newPanes,
        paneStatuses: newStatuses,
      };
    }

    case "SET_FOCUSED_PANE":
      // Defense-in-depth: ignore focus changes targeting a pane we don't know
      // about (race window between backend `pane-focused` event and the
      // structural state catching up). Listener also retries once after 100 ms.
      if (action.paneId && !state.panes[action.paneId]) return state;
      return { ...state, focused_pane_id: action.paneId };

    case "SET_PANE_PTY": {
      const pane = state.panes[action.paneId];
      if (!pane) return state;
      return {
        ...state,
        panes: {
          ...state.panes,
          [action.paneId]: { ...pane, pty_id: action.ptyId },
        },
      };
    }

    case "SET_PANE_TITLE": {
      const pane = state.panes[action.paneId];
      if (!pane) return state;
      const levels = [TitleLevel.Default, TitleLevel.Auto, TitleLevel.User];
      const newLevel = action.level || TitleLevel.Auto;
      const curLevel = pane.title_level || TitleLevel.Default;
      if (levels.indexOf(newLevel) < levels.indexOf(curLevel)) return state;
      const displayTitle = shortenTitle(action.title);
      const updatedPanes = {
        ...state.panes,
        [action.paneId]: {
          ...pane,
          title: action.title,
          title_level: newLevel,
        },
      };
      const updatedWorkspaces = state.workspaces.map((ws) => ({
        ...ws,
        tabs: ws.tabs.map((tab) => {
          const ids = collectPaneIds(tab.split_tree);
          if (ids.includes(action.paneId)) {
            return { ...tab, title: displayTitle };
          }
          return tab;
        }),
      }));
      return { ...state, panes: updatedPanes, workspaces: updatedWorkspaces };
    }

    case "UPDATE_SPLIT_RATIO": {
      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (ws.id !== action.workspaceId) return ws;
          return {
            ...ws,
            tabs: ws.tabs.map((tab) => {
              if (tab.id !== action.tabId) return tab;
              return {
                ...tab,
                split_tree: updateRatio(tab.split_tree, action.path, action.ratio),
              };
            }),
          };
        }),
      };
    }

    case "ADD_NOTIFICATION":
      return {
        ...state,
        notifications: [...state.notifications, action.notification],
        toasts: [...state.toasts, action.notification],
      };

    case "MARK_READ":
      return {
        ...state,
        notifications: state.notifications.map((n) =>
          n.id === action.id ? { ...n, read: true } : n,
        ),
      };

    case "MARK_ALL_READ":
      return {
        ...state,
        notifications: state.notifications.map((n) => ({ ...n, read: true })),
      };

    case "CLEAR_NOTIFICATIONS":
      return {
        ...state,
        notifications: [],
      };

    case "DISMISS_TOAST":
      return {
        ...state,
        toasts: state.toasts.filter((t) => t.id !== action.id),
      };

    case "SET_AGENT_TYPE": {
      const agentPane = state.panes[action.paneId];
      if (!agentPane) return state;
      return {
        ...state,
        panes: {
          ...state.panes,
          [action.paneId]: { ...agentPane, agent_type: action.agentType },
        },
      };
    }

    case "UPDATE_PANE_STATUS":
      return {
        ...state,
        paneStatuses: {
          ...state.paneStatuses,
          [action.paneId]: action.status,
        },
      };

    default:
      return state;
  }
}
