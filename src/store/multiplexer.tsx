import {
  createContext,
  useContext,
  useCallback,
  useReducer,
  useEffect,
  useRef,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { uid } from "../lib/uid";
import { setPendingCommand, setPendingCwd } from "../lib/pendingCommands";
import type {
  MultiplexerState,
  Workspace,
  Tab,
  Pane,
  SplitNode,
  Orientation,
  Notification,
  PaneStatus,
} from "../lib/types";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeBrowserPane(url: string): Pane {
  return {
    id: uid(),
    pty_id: null,
    pane_type: "Browser",
    title: "Browser",
    cwd: null,
    url,
  };
}

function makeTab(pane: Pane): Tab {
  return {
    id: uid(),
    title: "Terminal",
    split_tree: { type: "Leaf", pane_id: pane.id },
  };
}


function collectPaneIds(node: SplitNode): string[] {
  if (node.type === "Leaf") return [node.pane_id];
  return node.children.flatMap(collectPaneIds);
}

function splitLeaf(
  node: SplitNode,
  targetPaneId: string,
  newPaneId: string,
  orientation: Orientation,
): SplitNode {
  if (node.type === "Leaf") {
    if (node.pane_id === targetPaneId) {
      return {
        type: "Split",
        orientation,
        ratio: 0.5,
        children: [
          { type: "Leaf", pane_id: targetPaneId },
          { type: "Leaf", pane_id: newPaneId },
        ],
      };
    }
    return node;
  }
  return {
    ...node,
    children: node.children.map((c) =>
      splitLeaf(c, targetPaneId, newPaneId, orientation),
    ),
  };
}

function removeLeaf(node: SplitNode, targetPaneId: string): SplitNode | null {
  if (node.type === "Leaf") {
    return node.pane_id === targetPaneId ? null : node;
  }
  const filtered = node.children
    .map((c) => removeLeaf(c, targetPaneId))
    .filter((c): c is SplitNode => c !== null);
  if (filtered.length === 0) return null;
  if (filtered.length === 1) return filtered[0];
  return { ...node, children: filtered };
}

function updateRatio(
  node: SplitNode,
  path: number[],
  newRatio: number,
): SplitNode {
  if (node.type === "Leaf") return node;
  if (path.length === 0) {
    return { ...node, ratio: newRatio };
  }
  const [head, ...tail] = path;
  return {
    ...node,
    children: node.children.map((c, i) =>
      i === head ? updateRatio(c, tail, newRatio) : c,
    ),
  };
}

/** Shorten terminal title for display — extract process name from full path */
function shortenTitle(title: string): string {
  // If it looks like a file path (contains \ or / and ends with .exe or similar)
  const pathMatch = title.match(/[/\\]([^/\\]+?)(?:\.exe)?$/i);
  if (pathMatch) {
    return pathMatch[1];
  }
  // If title is too long, truncate with ellipsis
  if (title.length > 50) {
    return title.slice(0, 47) + "...";
  }
  return title;
}

// ---------------------------------------------------------------------------
// Extended state with notifications + pane statuses
// ---------------------------------------------------------------------------

interface ExtendedState extends MultiplexerState {
  notifications: Notification[];
  paneStatuses: Record<string, PaneStatus>;
  toasts: Notification[];
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

type Action =
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
  // Notification actions
  | { type: "ADD_NOTIFICATION"; notification: Notification }
  | { type: "MARK_READ"; id: string }
  | { type: "MARK_ALL_READ" }
  | { type: "CLEAR_NOTIFICATIONS" }
  | { type: "DISMISS_TOAST"; id: string }
  // Pane title — pinned=true from API/CLI rename (blocks OSC auto-rename)
  | { type: "SET_PANE_TITLE"; paneId: string; title: string; pinned?: boolean }
  // Agent detection
  | { type: "SET_AGENT_TYPE"; paneId: string; agentType: "claude" | null }
  // Pane status actions
  | { type: "UPDATE_PANE_STATUS"; paneId: string; status: PaneStatus };

function reducer(state: ExtendedState, action: Action): ExtendedState {
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
            // Preserve the existing split_tree (which has user-set ratios)
            // only if the pane structure hasn't changed
            const existingPaneIds = collectPaneIds(existingTab.split_tree).sort().join(",");
            const backendPaneIds = collectPaneIds(bTab.split_tree).sort().join(",");
            if (existingPaneIds === backendPaneIds) {
              return { ...bTab, split_tree: existingTab.split_tree };
            }
            return bTab;
          }),
        };
      });
      // Merge pane flags: pinned_title from backend OR frontend, agent_type from frontend
      const mergedPanes = { ...action.state.panes };
      for (const [id, backendPane] of Object.entries(mergedPanes)) {
        const existing = state.panes[id];
        const pinned = backendPane.pinned_title || existing?.pinned_title;
        const agentType = existing?.agent_type || backendPane.agent_type;
        if (pinned || agentType) {
          mergedPanes[id] = {
            ...backendPane,
            ...(pinned ? { pinned_title: true, title: backendPane.pinned_title ? backendPane.title : (existing?.title || backendPane.title) } : {}),
            ...(agentType ? { agent_type: agentType } : {}),
          };
        }
      }
      return {
        ...state,
        ...action.state,
        workspaces: mergedWorkspaces,
        panes: mergedPanes,
      };
    }

    case "ADD_WORKSPACE": {
      return {
        ...state,
        workspaces: [...state.workspaces, action.workspace],
        panes: { ...state.panes, [action.pane.id]: action.pane },
        active_workspace_id: action.workspace.id,
        focused_pane_id: action.pane.id,
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
      return {
        ...state,
        workspaces: remaining,
        panes: newPanes,
        paneStatuses: newStatuses,
        active_workspace_id:
          state.active_workspace_id === action.id
            ? (remaining[0]?.id ?? null)
            : state.active_workspace_id,
        focused_pane_id: removedPaneIds.includes(state.focused_pane_id ?? "")
          ? null
          : state.focused_pane_id,
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
      return {
        ...state,
        workspaces: state.workspaces.map((ws) =>
          ws.id === action.workspaceId
            ? { ...ws, tabs: [...ws.tabs, action.tab], active_tab_id: action.tab.id }
            : ws,
        ),
        panes: { ...state.panes, [action.pane.id]: action.pane },
        focused_pane_id: action.pane.id,
      };
    }

    case "CLOSE_TAB": {
      const newPanes = { ...state.panes };
      const newStatuses = { ...state.paneStatuses };
      for (const pid of action.removedPaneIds) {
        delete newPanes[pid];
        delete newStatuses[pid];
      }
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
        focused_pane_id: action.removedPaneIds.includes(
          state.focused_pane_id ?? "",
        )
          ? null
          : state.focused_pane_id,
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
        focused_pane_id: action.newPane.id,
      };
    }

    case "CLOSE_PANE": {
      const newPanes = { ...state.panes };
      delete newPanes[action.paneId];
      const newStatuses = { ...state.paneStatuses };
      delete newStatuses[action.paneId];
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
        focused_pane_id:
          state.focused_pane_id === action.paneId
            ? null
            : state.focused_pane_id,
      };
    }

    case "SET_FOCUSED_PANE":
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
      // If title is pinned (manually renamed) and this is an OSC auto-rename, skip
      if (pane.pinned_title && !action.pinned) return state;
      const displayTitle = shortenTitle(action.title);
      const updatedPanes = {
        ...state.panes,
        [action.paneId]: {
          ...pane,
          title: action.title,
          pinned_title: action.pinned || pane.pinned_title,
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

    // -- Notification reducers --

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

    // -- Pane status reducer --

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

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

interface MultiplexerContextType {
  state: ExtendedState;
  createWorkspace: (name: string) => void;
  setActiveWorkspace: (id: string) => void;
  closeWorkspace: (id: string) => void;
  renameWorkspace: (id: string, name: string) => void;
  moveTab: (tabId: string, fromWorkspaceId: string, toWorkspaceId: string) => void;
  createTab: (workspaceId: string, cwd?: string) => void;
  closeTab: (workspaceId: string, tabId: string) => void;
  setActiveTab: (workspaceId: string, tabId: string) => void;
  splitPane: (paneId: string, orientation: Orientation) => void;
  closePane: (paneId: string) => void;
  setFocusedPane: (paneId: string) => void;
  openBrowser: (url: string) => void;
  setPanePty: (paneId: string, ptyId: string) => void;
  updateSplitRatio: (workspaceId: string, tabId: string, path: number[], ratio: number) => void;
  // Notification actions
  addNotification: (notif: Omit<Notification, "id" | "timestamp" | "read">) => void;
  markRead: (id: string) => void;
  markAllRead: () => void;
  clearNotifications: () => void;
  dismissToast: (id: string) => void;
  // Pane title (from terminal OSC sequences)
  setPaneTitle: (paneId: string, title: string, pinned?: boolean) => void;
  setAgentType: (paneId: string, agentType: "claude" | null) => void;
  // Pane status
  updatePaneStatus: (paneId: string, status: PaneStatus) => void;
  // Spawn Claude in a new tab
  spawnClaudeTab: (workspaceId: string, skipPermissions: boolean, cwd?: string) => void;
}

const MultiplexerContext = createContext<MultiplexerContextType | null>(null);

export function useMultiplexer(): MultiplexerContextType {
  const ctx = useContext(MultiplexerContext);
  if (!ctx) throw new Error("useMultiplexer must be used within MultiplexerProvider");
  return ctx;
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

function buildInitialState(): ExtendedState {
  // Start empty — real state comes from Rust backend via mux_get_state
  return {
    workspaces: [],
    panes: {},
    active_workspace_id: null,
    focused_pane_id: null,
    notifications: [],
    paneStatuses: {},
    toasts: [],
  };
}

export function MultiplexerProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, null, buildInitialState);
  const stateRef = useRef(state);
  stateRef.current = state;

  // Load initial state from Rust backend (which already spawned PTY)
  useEffect(() => {
    invoke<{
      workspaces: Workspace[];
      panes: Record<string, Pane>;
      active_workspace_id: string | null;
      focused_pane_id: string | null;
    }>("mux_get_state")
      .then((backendState) => {
        if (backendState && backendState.workspaces.length > 0) {
          dispatch({
            type: "SET_STATE",
            state: {
              workspaces: backendState.workspaces,
              panes: backendState.panes,
              active_workspace_id: backendState.active_workspace_id,
              focused_pane_id: backendState.focused_pane_id,
            },
          });
        }
      })
      .catch((err) => {
        console.warn("Failed to load backend state, using local state:", err);
      });
  }, []);

  // Startup check: outdated skills → show action toast
  useEffect(() => {
    const checkSkills = async () => {
      try {
        const skills = await invoke<Array<{
          id: string; name: string; installed: boolean;
          update_available: boolean; revision: number; installed_revision: number;
        }>>("list_skills");

        for (const skill of skills) {
          if (skill.update_available) {
            dispatch({
              type: "ADD_NOTIFICATION",
              notification: {
                id: uid(),
                pane_id: null,
                workspace_id: null,
                title: `Skill update: ${skill.name}`,
                body: `v${skill.installed_revision} → v${skill.revision}`,
                level: "Info" as const,
                timestamp: Date.now(),
                read: false,
                sticky: true,
                actions: [
                  { label: "Update", type: "invoke" as const, value: "install_skill", args: { skillId: skill.id } },
                  { label: "Dismiss", type: "dismiss" as const },
                ],
              },
            });
          }
        }
      } catch (e) {
        console.warn("Skill check failed:", e);
      }
    };

    // Delay check so UI renders first
    const timer = setTimeout(checkSkills, 2000);
    return () => clearTimeout(timer);
  }, []);

  // Listen for Tauri events
  useEffect(() => {
    const unlisteners: (() => void)[] = [];

    listen<{
      pane_id: string | null;
      workspace_id: string | null;
      title: string;
      body: string | null;
      level: "Info" | "Warning" | "Error";
      actions?: Array<{ label: string; type: "invoke" | "url" | "dismiss"; value?: string; args?: Record<string, unknown> }>;
      sticky?: boolean;
    }>("notification-created", (event) => {
      dispatch({
        type: "ADD_NOTIFICATION",
        notification: {
          id: uid(),
          pane_id: event.payload.pane_id,
          workspace_id: event.payload.workspace_id,
          title: event.payload.title,
          body: event.payload.body,
          level: event.payload.level,
          timestamp: Date.now(),
          read: false,
          actions: event.payload.actions,
          sticky: event.payload.sticky,
        },
      });
    }).then((unlisten) => unlisteners.push(unlisten));

    listen<{ pane_id: string; status: PaneStatus }>(
      "pane-status-changed",
      (event) => {
        dispatch({
          type: "UPDATE_PANE_STATUS",
          paneId: event.payload.pane_id,
          status: event.payload.status,
        });
      },
    ).then((unlisten) => unlisteners.push(unlisten));

    // Listen for pinned pane title (from API/CLI rename — blocks OSC auto-rename)
    listen<{ pane_id: string; title: string }>("pane-title-pinned", (event) => {
      dispatch({
        type: "SET_PANE_TITLE",
        paneId: event.payload.pane_id,
        title: event.payload.title,
        pinned: true,
      });
    }).then((unlisten) => unlisteners.push(unlisten));

    // Listen for mux state changes from REST API / socket
    listen("mux-state-changed", () => {
      invoke<MultiplexerState>("mux_get_state")
        .then((s) => {
          if (s && s.workspaces.length > 0) {
            dispatch({ type: "SET_STATE", state: s });
            // After layout change (e.g. split via API), trigger pane resize
            // so all xterm instances refit to their new container size
            requestAnimationFrame(() => {
              window.dispatchEvent(new Event("codeterm-pane-resize"));
            });
          }
        })
        .catch(() => {});
    }).then((unlisten) => unlisteners.push(unlisten));

    // Auto-save session every 30 seconds
    const saveInterval = setInterval(() => {
      invoke("save_session").catch(() => {});
    }, 30_000);

    // Also save on window close / before unload
    const beforeUnload = () => { invoke("save_session").catch(() => {}); };
    window.addEventListener("beforeunload", beforeUnload);

    return () => {
      for (const unlisten of unlisteners) unlisten();
      clearInterval(saveInterval);
      window.removeEventListener("beforeunload", beforeUnload);
    };
  }, []);

  // Find the active workspace + tab for operations that need them
  const findContext = useCallback(
    (paneId: string) => {
      for (const ws of state.workspaces) {
        for (const tab of ws.tabs) {
          const ids = collectPaneIds(tab.split_tree);
          if (ids.includes(paneId)) {
            return { workspaceId: ws.id, tabId: tab.id };
          }
        }
      }
      return null;
    },
    [state.workspaces],
  );

  const createWorkspace = useCallback(async (name: string) => {
    try {
      await invoke("mux_create_workspace", { name });
      const s = await invoke<MultiplexerState>("mux_get_state");
      if (s && s.workspaces.length > 0) dispatch({ type: "SET_STATE", state: s });
    } catch (e) {
      console.error("Failed to create workspace:", e);
    }
  }, []);

  const setActiveWorkspace = useCallback((id: string) => {
    invoke("mux_set_active_workspace", { workspaceId: id }).catch(() => {});
    dispatch({ type: "SET_ACTIVE_WORKSPACE", id });
  }, []);

  const closeWorkspace = useCallback(
    (id: string) => {
      const ws = state.workspaces.find((w) => w.id === id);
      if (ws) {
        const paneIds = ws.tabs.flatMap((t) => collectPaneIds(t.split_tree));
        for (const pid of paneIds) {
          const pane = state.panes[pid];
          if (pane?.pty_id) {
            invoke("kill_pty", { ptyId: pane.pty_id }).catch(() => {});
          }
        }
      }
      invoke("mux_close_workspace", { workspaceId: id }).catch(() => {});
      dispatch({ type: "CLOSE_WORKSPACE", id });
    },
    [state],
  );

  const renameWorkspace = useCallback((id: string, name: string) => {
    invoke("mux_rename_workspace", { workspaceId: id, name }).catch(() => {});
    dispatch({ type: "RENAME_WORKSPACE", id, name });
  }, []);

  const moveTab = useCallback((tabId: string, fromWorkspaceId: string, toWorkspaceId: string) => {
    invoke("mux_move_tab", { tabId, fromWorkspaceId, toWorkspaceId }).catch(() => {});
    dispatch({ type: "MOVE_TAB", tabId, fromWorkspaceId, toWorkspaceId });
  }, []);

  const refreshFromBackend = useCallback(() => {
    invoke<MultiplexerState>("mux_get_state")
      .then((s) => {
        if (s && s.workspaces.length > 0) {
          dispatch({ type: "SET_STATE", state: s });
        }
      })
      .catch((e) => console.warn("Failed to refresh state:", e));
  }, []);

  const createTab = useCallback(async (workspaceId: string, cwd?: string) => {
    try {
      // 1. Switch to workspace (user action — navigate to where tab is created)
      invoke("mux_set_active_workspace", { workspaceId }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
      // 2. Create tab + pane in backend (pane has no PTY — TerminalPane will spawn it)
      const tabInfo = await invoke<{ id: string; title: string; pane_id: string }>("mux_create_tab", { workspaceId });
      // 3. Set cwd on pane — both pending map and backend
      if (cwd) {
        setPendingCwd(tabInfo.pane_id, cwd);
        await invoke("mux_update_pane_cwd", { paneId: tabInfo.pane_id, cwd });
      }
      // 4. Set active tab in backend
      await invoke("mux_set_active_tab", { workspaceId, tabId: tabInfo.id });
      // 5. Optimistically switch to the new tab before refresh
      dispatch({
        type: "SET_ACTIVE_TAB",
        workspaceId,
        tabId: tabInfo.id,
      });
      // 6. Refresh frontend — TerminalPane will see ptyId=null and spawn PTY itself
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to create tab:", e);
    }
  }, [refreshFromBackend]);

  const spawnClaudeTab = useCallback(async (workspaceId: string, skipPermissions: boolean, cwd?: string) => {
    try {
      invoke("mux_set_active_workspace", { workspaceId }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
      const tabInfo = await invoke<{ id: string; title: string; pane_id: string }>("mux_create_tab", { workspaceId });
      // Set cwd both on backend and in pending map (state may not be ready when PTY spawns)
      if (cwd) {
        setPendingCwd(tabInfo.pane_id, cwd);
        await invoke("mux_update_pane_cwd", { paneId: tabInfo.pane_id, cwd });
      }
      const cmd = skipPermissions ? "claude --dangerously-skip-permissions" : "claude";
      setPendingCommand(tabInfo.pane_id, cmd);
      await invoke("mux_set_active_tab", { workspaceId, tabId: tabInfo.id });
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId: tabInfo.id });
      console.log("[claude-spawn] refreshing frontend...");
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to spawn Claude tab:", e);
    }
  }, [refreshFromBackend]);

  const closeTab = useCallback(
    (workspaceId: string, tabId: string) => {
      const ws = state.workspaces.find((w) => w.id === workspaceId);
      const tab = ws?.tabs.find((t) => t.id === tabId);
      if (!tab) return;
      const removedPaneIds = collectPaneIds(tab.split_tree);
      // close_pane cascade-kills children PTYs and removes child tabs
      for (const pid of removedPaneIds) {
        invoke("mux_close_pane", { paneId: pid }).catch(() => {});
      }
      // Collect all cascaded child pane IDs for frontend cleanup
      const allRemoved = new Set(removedPaneIds);
      const collectChildren = (parentId: string) => {
        for (const [id, p] of Object.entries(state.panes)) {
          if (p.parent_pane_id === parentId && !allRemoved.has(id)) {
            allRemoved.add(id);
            collectChildren(id);
          }
        }
      };
      for (const pid of removedPaneIds) collectChildren(pid);
      dispatch({ type: "CLOSE_TAB", workspaceId, tabId, removedPaneIds: [...allRemoved] });
      // Refresh from backend to sync all cascade-closed tabs
      refreshFromBackend();
    },
    [state, refreshFromBackend],
  );

  const setActiveTab = useCallback((workspaceId: string, tabId: string) => {
    invoke("mux_set_active_tab", { workspaceId, tabId }).catch(() => {});
    dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId });
  }, []);

  const splitPane = useCallback(
    async (paneId: string, orientation: Orientation) => {
      try {
        // Use backend mux_split_pane which creates pane + spawns PTY + links + starts read loop
        await invoke<{ new_pane_id: string; new_pty_id: string }>("mux_split_pane", {
          paneId,
          orientation,
          cols: 80,
          rows: 24,
        });
        refreshFromBackend();
        // Trigger terminal refit after layout updates with new split dimensions
        requestAnimationFrame(() => {
          window.dispatchEvent(new CustomEvent("codeterm-pane-resize"));
        });
      } catch (e) {
        console.error("Failed to split pane:", e);
      }
    },
    [findContext],
  );

  const closePane = useCallback(
    (paneId: string) => {
      const ctx = findContext(paneId);
      if (!ctx) return;
      const pane = state.panes[paneId];
      if (pane?.pty_id) {
        invoke("kill_pty", { ptyId: pane.pty_id }).catch(() => {});
      }
      // Remove pane from backend state so it doesn't reappear on refresh
      invoke("mux_close_pane", { paneId }).catch(() => {});
      const ws = state.workspaces.find((w) => w.id === ctx.workspaceId);
      const tab = ws?.tabs.find((t) => t.id === ctx.tabId);
      if (tab) {
        const allPanes = collectPaneIds(tab.split_tree);
        if (allPanes.length <= 1) {
          // Also close tab in backend when it's the last pane
          invoke("mux_close_tab", { workspaceId: ctx.workspaceId, tabId: ctx.tabId }).catch(() => {});
          dispatch({
            type: "CLOSE_TAB",
            workspaceId: ctx.workspaceId,
            tabId: ctx.tabId,
            removedPaneIds: [paneId],
          });
          return;
        }
      }
      dispatch({
        type: "CLOSE_PANE",
        workspaceId: ctx.workspaceId,
        tabId: ctx.tabId,
        paneId,
      });
      // Trigger refit on remaining pane(s) after layout change
      requestAnimationFrame(() => {
        window.dispatchEvent(new CustomEvent("codeterm-pane-resize"));
      });
    },
    [findContext, state],
  );

  const openBrowser = useCallback(
    (url: string) => {
      const s = stateRef.current;
      const activeWs = s.workspaces.find((ws) => ws.id === s.active_workspace_id);
      if (!activeWs) return;

      const focusedId = s.focused_pane_id;
      const browserPane = makeBrowserPane(url);

      if (focusedId) {
        // Split the focused pane horizontally to show browser alongside
        const ctx = (() => {
          for (const ws of s.workspaces) {
            for (const tab of ws.tabs) {
              const ids = collectPaneIds(tab.split_tree);
              if (ids.includes(focusedId)) {
                return { workspaceId: ws.id, tabId: tab.id };
              }
            }
          }
          return null;
        })();

        if (ctx) {
          dispatch({
            type: "SPLIT_PANE",
            workspaceId: ctx.workspaceId,
            tabId: ctx.tabId,
            targetPaneId: focusedId,
            newPane: browserPane,
            orientation: "Horizontal",
          });
          return;
        }
      }

      // Fallback: open as new tab
      const tab = makeTab(browserPane);
      dispatch({ type: "ADD_TAB", workspaceId: activeWs.id, tab, pane: browserPane });
    },
    [],
  );

  const setFocusedPane = useCallback((paneId: string) => {
    dispatch({ type: "SET_FOCUSED_PANE", paneId });
  }, []);

  const setPanePty = useCallback((paneId: string, ptyId: string) => {
    dispatch({ type: "SET_PANE_PTY", paneId, ptyId });
  }, []);

  const updateSplitRatio = useCallback(
    (workspaceId: string, tabId: string, path: number[], ratio: number) => {
      dispatch({ type: "UPDATE_SPLIT_RATIO", workspaceId, tabId, path, ratio });
    },
    [],
  );

  // Notification actions
  const addNotification = useCallback(
    (notif: Omit<Notification, "id" | "timestamp" | "read">) => {
      dispatch({
        type: "ADD_NOTIFICATION",
        notification: {
          ...notif,
          id: uid(),
          timestamp: Date.now(),
          read: false,
        },
      });
    },
    [],
  );

  const markRead = useCallback((id: string) => {
    dispatch({ type: "MARK_READ", id });
  }, []);

  const markAllRead = useCallback(() => {
    dispatch({ type: "MARK_ALL_READ" });
  }, []);

  const clearNotifications = useCallback(() => {
    dispatch({ type: "CLEAR_NOTIFICATIONS" });
  }, []);

  const dismissToast = useCallback((id: string) => {
    dispatch({ type: "DISMISS_TOAST", id });
  }, []);

  const setPaneTitle = useCallback((paneId: string, title: string, pinned?: boolean) => {
    dispatch({ type: "SET_PANE_TITLE", paneId, title, pinned });
  }, []);

  const setAgentType = useCallback((paneId: string, agentType: "claude" | null) => {
    dispatch({ type: "SET_AGENT_TYPE", paneId, agentType });
  }, []);

  const updatePaneStatus = useCallback((paneId: string, status: PaneStatus) => {
    dispatch({ type: "UPDATE_PANE_STATUS", paneId, status });
  }, []);

  const value: MultiplexerContextType = {
    state,
    createWorkspace,
    setActiveWorkspace,
    closeWorkspace,
    renameWorkspace,
    moveTab,
    createTab,
    closeTab,
    setActiveTab,
    splitPane,
    closePane,
    setFocusedPane,
    openBrowser,
    setPanePty,
    updateSplitRatio,
    addNotification,
    markRead,
    markAllRead,
    clearNotifications,
    dismissToast,
    setPaneTitle,
    setAgentType,
    updatePaneStatus,
    spawnClaudeTab,
  };

  return (
    <MultiplexerContext.Provider value={value}>
      {children}
    </MultiplexerContext.Provider>
  );
}
