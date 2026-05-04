import {
  useCallback,
  useReducer,
  useEffect,
  useRef,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { uid } from "../../lib/uid";
import { setPendingCommand, setPendingCwd } from "../../lib/pendingCommands";
import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Workspace,
  Pane,
  Notification,
  PaneStatus,
} from "../../lib/types";
import {
  collectPaneIds,
  makeBrowserPane,
  makeTab,
  type ExtendedState,
} from "./tree";
import { reducer } from "./reducer";
import { MultiplexerContext, type MultiplexerContextType } from "./context";

export { useMultiplexer } from "./context";
export { highestTitleLevel } from "./tree";

function buildInitialState(): ExtendedState {
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

    listen<{ pane_id: string; title: string }>("pane-title-pinned", (event) => {
      dispatch({
        type: "SET_PANE_TITLE",
        paneId: event.payload.pane_id,
        title: event.payload.title,
        level: TitleLevel.User,
      });
    }).then((unlisten) => unlisteners.push(unlisten));

    listen<{
      current_version: string;
      latest_version: string;
      release_url: string;
      release_notes: string | null;
      update_command: string | null;
    }>("update-available", (event) => {
      const { latest_version, release_url, update_command } = event.payload;
      const body = update_command
        ? `Run: ${update_command}`
        : "Click to view release notes";
      dispatch({
        type: "ADD_NOTIFICATION",
        notification: {
          id: `update-${latest_version}`,
          pane_id: null,
          workspace_id: null,
          title: `CodeTerm v${latest_version} available`,
          body,
          level: "Info",
          timestamp: Date.now(),
          read: false,
          sticky: true,
          actions: [
            { label: "View Release", type: "url", value: release_url },
            { label: "Dismiss", type: "dismiss" },
          ],
        },
      });
    }).then((unlisten) => unlisteners.push(unlisten));

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

    const saveInterval = setInterval(() => {
      invoke("save_session").catch(() => {});
    }, 30_000);

    const beforeUnload = () => { invoke("save_session").catch(() => {}); };
    window.addEventListener("beforeunload", beforeUnload);

    return () => {
      for (const unlisten of unlisteners) unlisten();
      clearInterval(saveInterval);
      window.removeEventListener("beforeunload", beforeUnload);
    };
  }, []);

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
      invoke("mux_set_active_workspace", { workspaceId }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
      const tabInfo = await invoke<{ id: string; title: string; pane_id: string }>("mux_create_tab", { workspaceId });
      const ws = stateRef.current.workspaces.find((w) => w.id === workspaceId);
      const effectiveCwd = cwd || ws?.cwd || undefined;
      if (effectiveCwd) {
        setPendingCwd(tabInfo.pane_id, effectiveCwd);
        await invoke("mux_update_pane_cwd", { paneId: tabInfo.pane_id, cwd: effectiveCwd });
      }
      await invoke("mux_set_active_tab", { workspaceId, tabId: tabInfo.id });
      dispatch({
        type: "SET_ACTIVE_TAB",
        workspaceId,
        tabId: tabInfo.id,
      });
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to create tab:", e);
    }
  }, [refreshFromBackend]);

  const createWorkspace = useCallback(async (name: string, cwd?: string) => {
    try {
      // Backend `service::create_workspace` normalizes the cwd and reuses an
      // existing workspace with the same folder (spawning a new tab there
      // instead of duplicating).
      await invoke("mux_create_workspace", { name, cwd: cwd ?? null });
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

  const spawnAgentTab = useCallback(async (workspaceId: string, agentId: string, skipPermissions: boolean, cwd?: string) => {
    try {
      invoke("mux_set_active_workspace", { workspaceId }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
      const tabInfo = await invoke<{ id: string; title: string; pane_id: string }>("mux_create_tab", { workspaceId });
      if (cwd) {
        setPendingCwd(tabInfo.pane_id, cwd);
        await invoke("mux_update_pane_cwd", { paneId: tabInfo.pane_id, cwd });
      }
      const cmd = await invoke<string>("build_agent_launch_command", { agentId, skipPermissions });
      setPendingCommand(tabInfo.pane_id, cmd);
      await invoke("mux_set_active_tab", { workspaceId, tabId: tabInfo.id });
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId: tabInfo.id });
      refreshFromBackend();
    } catch (e) {
      console.error(`Failed to spawn ${agentId} tab:`, e);
    }
  }, [refreshFromBackend]);

  const spawnClaudeTab = useCallback(async (workspaceId: string, skipPermissions: boolean, cwd?: string) => {
    spawnAgentTab(workspaceId, "claude", skipPermissions, cwd);
  }, [spawnAgentTab]);

  const closeTab = useCallback(
    (workspaceId: string, tabId: string) => {
      const ws = state.workspaces.find((w) => w.id === workspaceId);
      const tab = ws?.tabs.find((t) => t.id === tabId);
      if (!tab) return;
      const removedPaneIds = collectPaneIds(tab.split_tree);
      for (const pid of removedPaneIds) {
        invoke("mux_close_pane", { paneId: pid }).catch(() => {});
      }
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
      refreshFromBackend();
    },
    [state, refreshFromBackend],
  );

  const setActiveTab = useCallback((workspaceId: string, tabId: string) => {
    invoke("mux_set_active_tab", { workspaceId, tabId }).catch(() => {});
    dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId });
  }, []);

  const closePane = useCallback(
    (paneId: string) => {
      const ctx = findContext(paneId);
      if (!ctx) return;
      const pane = state.panes[paneId];
      if (pane?.pty_id) {
        invoke("kill_pty", { ptyId: pane.pty_id }).catch(() => {});
      }
      invoke("mux_close_pane", { paneId }).catch(() => {});
      const ws = state.workspaces.find((w) => w.id === ctx.workspaceId);
      const tab = ws?.tabs.find((t) => t.id === ctx.tabId);
      if (tab) {
        const allPanes = collectPaneIds(tab.split_tree);
        if (allPanes.length <= 1) {
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

  const setPaneTitle = useCallback((paneId: string, title: string, level?: TitleLevel) => {
    dispatch({ type: "SET_PANE_TITLE", paneId, title, level });
  }, []);

  const setAgentType = useCallback((paneId: string, agentType: string | null) => {
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
    spawnAgentTab,
  };

  return (
    <MultiplexerContext.Provider value={value}>
      {children}
    </MultiplexerContext.Provider>
  );
}
