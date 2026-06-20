import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Workspace,
  Tab,
  Pane,
  Notification,
  PaneStatus,
} from "../../lib/types";
import {
  highestTitleLevel,
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
      type: "CLOSE_PANE";
      workspaceId: string;
      tabId: string;
      paneId: string;
    }
  | { type: "SET_FOCUSED_PANE"; paneId: string }
  | { type: "SET_PANE_PTY"; paneId: string; ptyId: string }
  | { type: "ADD_NOTIFICATION"; notification: Notification }
  | { type: "UPSERT_TOAST"; toast: Notification }
  | { type: "MARK_READ"; id: string }
  | { type: "MARK_ALL_READ" }
  | { type: "CLEAR_NOTIFICATIONS" }
  | { type: "DISMISS_TOAST"; id: string }
  | { type: "SET_PANE_TITLE"; paneId: string; title: string; level?: TitleLevel }
  | { type: "SET_TAB_TITLE"; workspaceId: string; tabId: string; title: string }
  | { type: "SET_AGENT_TYPE"; paneId: string; agentType: string | null }
  | { type: "UPDATE_PANE_STATUS"; paneId: string; status: PaneStatus };

export function reducer(state: ExtendedState, action: Action): ExtendedState {
  switch (action.type) {
    case "SET_STATE": {
      // Monotonic guard: drop a stale snapshot (older generation) that resolved
      // after a newer one — kills the spawn-then-blink race where a pre-pane
      // `mux_get_state` clobbers the just-created tab.
      const incomingGen = action.state.generation;
      if (typeof incomingGen === "number" && incomingGen <= state.generation) {
        return state;
      }
      // Backend is authoritative; one pane per tab means no split ratio.
      const mergedWorkspaces = action.state.workspaces;
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
      const activeTabIds: Record<string, string> = {};
      for (const w of mergedWorkspaces) {
        if (w.active_tab_id) activeTabIds[w.id] = w.active_tab_id;
      }
      // Snapshots now carry `focused_pane_id`. Adopt it when it names a pane
      // present in the merged set (initial hydrate / restore where the separate
      // `pane-focused` event may be missed); otherwise keep the current focus —
      // never clobber a valid focus with a stale/null value naming nothing.
      const incomingFocus = action.state.focused_pane_id;
      const focused_pane_id =
        incomingFocus && mergedPanes[incomingFocus]
          ? incomingFocus
          : state.focused_pane_id;
      return {
        ...state,
        workspaces: mergedWorkspaces,
        panes: mergedPanes,
        focused_pane_id,
        active_workspace_id: action.state.active_workspace_id,
        toasts: [...state.toasts, ...newToasts],
        activeTabIds,
        // Backend-authoritative, sqlite-sourced closed-tab history; carry it
        // from the incoming snapshot or the list silently stays empty.
        recently_closed: action.state.recently_closed ?? state.recently_closed,
        generation:
          typeof incomingGen === "number" ? incomingGen : state.generation,
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
      const removedPaneIds = ws ? ws.tabs.map((t) => t.pane_id) : [];
      const newPanes = { ...state.panes };
      const newStatuses = { ...state.paneStatuses };
      for (const pid of removedPaneIds) {
        delete newPanes[pid];
        delete newStatuses[pid];
      }
      const remaining = state.workspaces.filter((w) => w.id !== action.id);
      const newActiveTabIds = { ...state.activeTabIds };
      delete newActiveTabIds[action.id];
      // keep focused_pane_id untouched; close_workspace emits fallback focus separately.
      return {
        ...state,
        workspaces: remaining,
        panes: newPanes,
        paneStatuses: newStatuses,
        activeTabIds: newActiveTabIds,
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

      const newActiveTabIds = { ...state.activeTabIds };
      const fromActiveTabId = newActiveTabIds[action.fromWorkspaceId];
      if (fromActiveTabId === action.tabId) {
        const remaining = fromWs.tabs.filter((t) => t.id !== action.tabId);
        const fallback = remaining[remaining.length - 1]?.id;
        if (fallback) {
          newActiveTabIds[action.fromWorkspaceId] = fallback;
        } else {
          delete newActiveTabIds[action.fromWorkspaceId];
        }
      }
      newActiveTabIds[action.toWorkspaceId] = action.tabId;

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
        activeTabIds: newActiveTabIds,
        active_workspace_id: action.toWorkspaceId,
      };
    }

    case "ADD_TAB": {
      // Caller decides focus on tab creation; don't auto-focus here.
      return {
        ...state,
        workspaces: state.workspaces.map((ws) =>
          ws.id === action.workspaceId
            ? { ...ws, tabs: [...ws.tabs, action.tab], active_tab_id: action.tab.id }
            : ws,
        ),
        panes: { ...state.panes, [action.pane.id]: action.pane },
        activeTabIds: { ...state.activeTabIds, [action.workspaceId]: action.tab.id },
      };
    }

    case "CLOSE_TAB": {
      const newPanes = { ...state.panes };
      const newStatuses = { ...state.paneStatuses };
      for (const pid of action.removedPaneIds) {
        delete newPanes[pid];
        delete newStatuses[pid];
      }
      const newActiveTabIds = { ...state.activeTabIds };
      // focused_pane_id intentionally untouched — see CLOSE_WORKSPACE comment.
      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (ws.id !== action.workspaceId) return ws;
          const remaining = ws.tabs.filter((t) => t.id !== action.tabId);
          const currentActive = newActiveTabIds[action.workspaceId];
          if (currentActive === action.tabId) {
            const fallback = remaining[remaining.length - 1]?.id;
            if (fallback) {
              newActiveTabIds[action.workspaceId] = fallback;
            } else {
              delete newActiveTabIds[action.workspaceId];
            }
          }
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
        activeTabIds: newActiveTabIds,
      };
    }

    case "SET_ACTIVE_TAB":
      return {
        ...state,
        activeTabIds: { ...state.activeTabIds, [action.workspaceId]: action.tabId },
      };

    case "CLOSE_PANE": {
      const newPanes = { ...state.panes };
      delete newPanes[action.paneId];
      const newStatuses = { ...state.paneStatuses };
      delete newStatuses[action.paneId];
      // keep focused_pane_id untouched (see CLOSE_WORKSPACE). One pane per tab.
      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (ws.id !== action.workspaceId) return ws;
          return {
            ...ws,
            tabs: ws.tabs.filter((tab) => tab.id !== action.tabId),
          };
        }),
        panes: newPanes,
        paneStatuses: newStatuses,
      };
    }

    case "SET_FOCUSED_PANE":
      // Ignore focus updates for unknown panes while state merge is in flight.
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
      return {
        ...state,
        panes: {
          ...state.panes,
          [action.paneId]: {
            ...pane,
            title: action.title,
            title_level: newLevel,
          },
        },
      };
    }

    case "SET_TAB_TITLE": {
      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (ws.id !== action.workspaceId) return ws;
          return {
            ...ws,
            tabs: ws.tabs.map((tab) =>
              tab.id === action.tabId ? { ...tab, title: action.title } : tab
            ),
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

    case "UPSERT_TOAST": {
      const existingIndex = action.toast.key
        ? state.toasts.findIndex((toast) => toast.key === action.toast.key)
        : state.toasts.findIndex((toast) => toast.id === action.toast.id);
      if (existingIndex === -1) {
        return { ...state, toasts: [...state.toasts, action.toast] };
      }
      const toasts = [...state.toasts];
      toasts[existingIndex] = action.toast;
      return { ...state, toasts };
    }

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
