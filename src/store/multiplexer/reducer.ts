import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Workspace,
  Tab,
  Pane,
  Notification,
  PaneStatus,
  BootStage,
} from "../../lib/types";
import {
  highestTitleLevel,
  reconcileBackendWorkspaceTabs,
  type ExtendedState,
} from "./tree";

export type Action =
  | { type: "SET_STATE"; state: MultiplexerState }
  | { type: "ADD_WORKSPACE"; workspace: Workspace; pane: Pane }
  | { type: "SET_ACTIVE_WORKSPACE"; id: string }
  | { type: "CLOSE_WORKSPACE"; id: string }
  | { type: "RENAME_WORKSPACE"; id: string; name: string }
  | { type: "MOVE_TAB"; tabId: string; fromWorkspaceId: string; toWorkspaceId: string }
  | { type: "REORDER_TABS"; orderedTabIds: string[] }
  | { type: "REORDER_WORKSPACES"; orderedWorkspaceIds: string[] }
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
  | { type: "HYDRATE_NOTIFICATIONS"; notifications: Notification[] }
  | { type: "UPSERT_TOAST"; toast: Notification }
  | { type: "MARK_READ"; id: string }
  | { type: "MARK_ALL_READ" }
  | { type: "DELETE_NOTIFICATION"; id: string }
  | { type: "CLEAR_NOTIFICATIONS" }
  | { type: "DISMISS_TOAST"; id: string }
  | { type: "SET_PANE_TITLE"; paneId: string; title: string; level?: TitleLevel }
  | { type: "SET_AGENT_TYPE"; paneId: string; agentType: string | null }
  | { type: "UPDATE_PANE_STATUS"; paneId: string; status: PaneStatus }
  | { type: "UPDATE_BOOT_STAGE"; paneId: string; stage: BootStage; reason?: string | null }
  | { type: "REMOVE_PENDING_CLOSE"; paneIds: string[] }
  | { type: "RESET_PEER_CONTEXT" }
  | { type: "RESET_GENERATION" };

/**
 * Merge a fresh `pane_statuses` snapshot into the known map, then drop the
 * synthetic "Waiting" the backend derives from a pane's `pending_prompt` once
 * that prompt is gone and the fresh snapshot doesn't re-affirm any status for
 * the pane. Plain merging alone leaves "Waiting" stuck forever for a pane
 * with no attached PTY (nothing for `snapshot_with_status` to report once
 * `pending_prompt` clears, so the key is simply absent from every future
 * snapshot) — e.g. a hook-driven attention prompt on a pane that hasn't
 * opened a real terminal yet. Any OTHER status (real Idle/Working/Dead, or a
 * re-affirmed "Waiting" from a genuine PTY-detected permission prompt) is
 * left untouched since the fresh snapshot explicitly carries it.
 */
function reconcilePaneStatuses(
  prev: Record<string, PaneStatus>,
  incoming: Record<string, PaneStatus> | undefined,
  panes: Record<string, Pane>,
): Record<string, PaneStatus> {
  const merged = incoming ? { ...prev, ...incoming } : { ...prev };
  for (const id of Object.keys(merged)) {
    if (
      merged[id] === "Waiting" &&
      !panes[id]?.pending_prompt &&
      !incoming?.[id]
    ) {
      delete merged[id];
    }
  }
  return merged;
}

function addPendingCloses(current: string[], paneIds: string[]): string[] {
  if (paneIds.length === 0) return current;
  const next = new Set(current);
  for (const id of paneIds) next.add(id);
  return next.size === current.length ? current : [...next];
}

export function reducer(state: ExtendedState, action: Action): ExtendedState {
  switch (action.type) {
    case "RESET_PEER_CONTEXT":
      return {
        ...state,
        workspaces: [],
        panes: {},
        teams: [],
        active_workspace_id: null,
        focused_pane_id: null,
        paneStatuses: {},
        activeTabIds: {},
        generation: -1,
        pendingCloses: [],
      };

    case "SET_STATE": {
      const incomingGen = action.state.generation;
      if (typeof incomingGen === "number" && incomingGen <= state.generation) {
        return state;
      }
      // This snapshot is accepted. Apply optimistic-close tombstones now, so a
      // tombstone clears only against a snapshot the generation guard accepted.
      const stillPending: string[] = [];
      const maskedPaneIds = new Set<string>();
      for (const paneId of state.pendingCloses) {
        const present =
          Boolean(action.state.panes[paneId]) ||
          action.state.workspaces.some((w) => w.tabs.some((t) => t.pane_id === paneId));
        if (present) {
          stillPending.push(paneId);
          maskedPaneIds.add(paneId);
        }
      }
      const sourcePanes = maskedPaneIds.size
        ? Object.fromEntries(
            Object.entries(action.state.panes).filter(([id]) => !maskedPaneIds.has(id)),
          )
        : action.state.panes;
      const sourceWorkspaces = maskedPaneIds.size
        ? action.state.workspaces.map((w) => ({
            ...w,
            tabs: w.tabs.filter((t) => !maskedPaneIds.has(t.pane_id)),
          }))
        : action.state.workspaces;
      // Backend is authoritative; one pane per tab means no split ratio.
      const mergedPanes = { ...sourcePanes };
      const mergedWorkspaces = reconcileBackendWorkspaceTabs(sourceWorkspaces, mergedPanes);
      for (const [id, backendPane] of Object.entries(mergedPanes)) {
        const existing = state.panes[id];
        const agentType = existing?.agent_type || backendPane.agent_type;
        const backendLevel = backendPane.title_level || TitleLevel.Default;
        const frontendLevel = existing?.title_level || TitleLevel.Default;
        const highestLevel = highestTitleLevel(backendLevel, frontendLevel);
        const keepFrontendTitle =
          existing &&
          existing.title_pending_local === true &&
          highestLevel === frontendLevel &&
          frontendLevel !== TitleLevel.Auto;
        const title = keepFrontendTitle ? existing.title : backendPane.title;
        // Backend has caught up once its title matches what we're keeping;
        // otherwise the local rename is still in flight.
        const title_pending_local = keepFrontendTitle && backendPane.title !== existing.title;
        mergedPanes[id] = {
          ...backendPane,
          title_level: highestLevel as TitleLevel,
          title,
          title_pending_local,
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
      // Preserve prior focus only while its pane still exists.
      const incomingFocus = action.state.focused_pane_id;
      const focused_pane_id =
        incomingFocus && mergedPanes[incomingFocus]
          ? incomingFocus
          : state.focused_pane_id && mergedPanes[state.focused_pane_id]
            ? state.focused_pane_id
            : null;
      return {
        ...state,
        workspaces: mergedWorkspaces,
        panes: mergedPanes,
        teams: action.state.teams ?? [],
        focused_pane_id,
        active_workspace_id: action.state.active_workspace_id,
        toasts: [...state.toasts, ...newToasts],
        activeTabIds,
        // Prefer the authoritative recently closed list when it is present.
        recently_closed: action.state.recently_closed ?? state.recently_closed,
        // Keep newer live status events when a snapshot omits them.
        paneStatuses: reconcilePaneStatuses(
          state.paneStatuses,
          action.state.pane_statuses,
          mergedPanes,
        ),
        generation:
          typeof incomingGen === "number" ? incomingGen : state.generation,
        pendingCloses: stillPending,
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
        pendingCloses: addPendingCloses(state.pendingCloses, removedPaneIds),
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

    case "REORDER_TABS": {
      const orderById = new Map(action.orderedTabIds.map((id, i) => [id, i]));
      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (!ws.tabs.some((t) => orderById.has(t.id))) return ws;
          return {
            ...ws,
            tabs: ws.tabs.map((t) =>
              orderById.has(t.id) ? { ...t, sort_order: orderById.get(t.id)! } : t,
            ),
          };
        }),
      };
    }

    case "REORDER_WORKSPACES": {
      const orderById = new Map(action.orderedWorkspaceIds.map((id, i) => [id, i]));
      return {
        ...state,
        workspaces: state.workspaces.map((ws) =>
          orderById.has(ws.id) ? { ...ws, sort_order: orderById.get(ws.id)! } : ws,
        ),
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
      const removedPaneIds = new Set(action.removedPaneIds);
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
          const removedTabIds = new Set(
            ws.tabs.filter((t) => t.id === action.tabId || removedPaneIds.has(t.pane_id)).map((t) => t.id),
          );
          const remaining = ws.tabs.filter((t) => !removedTabIds.has(t.id));
          const currentActive = newActiveTabIds[action.workspaceId];
          if (currentActive && removedTabIds.has(currentActive)) {
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
              ws.active_tab_id && removedTabIds.has(ws.active_tab_id)
                ? (remaining[remaining.length - 1]?.id ?? null)
                : ws.active_tab_id,
          };
        }),
        panes: newPanes,
        paneStatuses: newStatuses,
        activeTabIds: newActiveTabIds,
        pendingCloses: addPendingCloses(state.pendingCloses, action.removedPaneIds),
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
        pendingCloses: addPendingCloses(state.pendingCloses, [action.paneId]),
      };
    }

    case "REMOVE_PENDING_CLOSE": {
      if (action.paneIds.length === 0) return state;
      const remove = new Set(action.paneIds);
      const next = state.pendingCloses.filter((id) => !remove.has(id));
      return next.length === state.pendingCloses.length
        ? state
        : { ...state, pendingCloses: next };
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
      const levels = [TitleLevel.Default, TitleLevel.Auto, TitleLevel.Agent, TitleLevel.User];
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
            title_pending_local: true,
          },
        },
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

    case "HYDRATE_NOTIFICATIONS": {
      const notifications = new Map(action.notifications.map((notification) => [notification.id, notification]));
      for (const notification of state.notifications) notifications.set(notification.id, notification);
      return { ...state, notifications: [...notifications.values()] };
    }

    case "DELETE_NOTIFICATION":
      return {
        ...state,
        notifications: state.notifications.filter((n) => n.id !== action.id),
        toasts: state.toasts.filter((n) => n.id !== action.id),
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

    case "UPDATE_BOOT_STAGE": {
      const pane = state.panes[action.paneId];
      if (!pane) return state;
      return {
        ...state,
        panes: {
          ...state.panes,
          [action.paneId]: {
            ...pane,
            boot_stage: action.stage,
            boot_failure_reason: action.stage === "spawn_failed" ? (action.reason ?? null) : null,
          },
        },
      };
    }

    case "RESET_GENERATION":
      return state.generation === 0 ? state : { ...state, generation: 0 };

    default:
      return state;
  }
}
