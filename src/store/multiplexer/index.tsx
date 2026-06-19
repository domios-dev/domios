import {
  useCallback,
  useMemo,
  useReducer,
  useEffect,
  useRef,
  type ReactNode,
} from "react";
import { invoke, reopenClosedPane, restoreOrphanedPane } from "../../lib/bridge";
import { listen } from "../../lib/bridge";
import { IS_WEB, webJson } from "../../lib/webApi";
import { uid } from "../../lib/uid";
import { setPendingCommand, setPendingCwd } from "../../lib/pendingCommands";
import { loadDefaultNewAgentPaneChat } from "../../lib/chatUiMode";
import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Workspace,
  Pane,
  Notification,
  PaneStatus,
  AtomicSpawn,
} from "../../lib/types";
import {
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
    activeTabIds: {},
    generation: 0,
  };
}

export function MultiplexerProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, null, buildInitialState);
  const stateRef = useRef(state);
  stateRef.current = state;
  // Per-button in-flight guard: a 2nd click while a spawn is pending is ignored
  // so one click = exactly one tab (no duplicate on double-fire).
  const spawnInFlight = useRef<Set<string>>(new Set());

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
    loadDefaultNewAgentPaneChat().catch(() => {});
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
        if (!event.payload) return;
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

    listen<MultiplexerState>("mux-state-changed", (event) => {
      const s = event.payload;
      if (s && s.workspaces.length > 0) {
        dispatch({ type: "SET_STATE", state: s });
        // Resize panes after layout changes so xterms fit new container sizes.
        requestAnimationFrame(() => {
          window.dispatchEvent(new Event("codeterm-pane-resize"));
        });
      }
    }).then((unlisten) => unlisteners.push(unlisten));

    const refreshMuxState = () => refreshFromBackend();
    window.addEventListener("codeterm-refresh-mux-state", refreshMuxState);
    unlisteners.push(() => window.removeEventListener("codeterm-refresh-mux-state", refreshMuxState));

    // Backend owns `focused_pane_id`; background events don't emit `pane-focused`.
    // Focus updates come only from CLI/REST or service fallback when the pane closes.
    listen<{ pane_id: string }>("pane-focused", (event) => {
      const paneId = event.payload.pane_id;
      if (stateRef.current.panes[paneId]) {
        dispatch({ type: "SET_FOCUSED_PANE", paneId });
      } else {
        // Focus may arrive before full state merge; retry once after a short delay.
        setTimeout(() => {
          if (stateRef.current.panes[paneId]) {
            dispatch({ type: "SET_FOCUSED_PANE", paneId });
          }
        }, 100);
      }
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
      for (const ws of stateRef.current.workspaces) {
        for (const tab of ws.tabs) {
          if (tab.pane_id === paneId) {
            return { workspaceId: ws.id, tabId: tab.id };
          }
        }
      }
      return null;
    },
    [],
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
      const tabInfo = await invoke<{ id: string; title: string; pane_id: string }>("mux_create_tab", { workspaceId, activate: true });
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
      // Explicit New Tab click focuses the new pane; background spawns don't.
      invoke("mux_set_focused_pane", { paneId: tabInfo.pane_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to create tab:", e);
    }
  }, [refreshFromBackend]);

  const reopenClosed = useCallback(async (workspaceId: string, index = 0) => {
    try {
      invoke("mux_set_active_workspace", { workspaceId }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
      const tabInfo = await reopenClosedPane(workspaceId, index);
      await invoke("mux_set_active_tab", { workspaceId, tabId: tabInfo.id }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId: tabInfo.id });
      invoke("mux_set_focused_pane", { paneId: tabInfo.pane_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to reopen closed pane:", e);
    }
  }, [refreshFromBackend]);

  const restoreOrphaned = useCallback(async (paneId: string) => {
    try {
      const result = await restoreOrphanedPane(paneId);
      invoke("mux_set_active_workspace", { workspaceId: result.workspace_id }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: result.workspace_id });
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId: result.workspace_id, tabId: result.tab_id });
      invoke("mux_set_focused_pane", { paneId: result.pane_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to restore orphaned pane:", e);
    }
  }, [refreshFromBackend]);

  const createWorkspace = useCallback(async (name: string, cwd?: string) => {
    try {
      // Reuse existing workspace for the same cwd; avoid duplicate tabs.
      await invoke("mux_create_workspace", { name, cwd: cwd ?? null, activate: true });
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
      const { workspaces, panes } = stateRef.current;
      const ws = workspaces.find((w) => w.id === id);
      if (ws) {
        const paneIds = ws.tabs.map((t) => t.pane_id);
        for (const pid of paneIds) {
          const pane = panes[pid];
          if (pane?.pty_id) {
            invoke("kill_pty", { ptyId: pane.pty_id }).catch(() => {});
          }
        }
      }
      invoke("mux_close_workspace", { workspaceId: id }).catch(() => {});
      dispatch({ type: "CLOSE_WORKSPACE", id });
    },
    [],
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
    // Web UI delegates full agent spawn to the daemon (REST path, unchanged).
    if (IS_WEB) {
      try {
        await webJson<{ pane_id: string }>("POST", "/api/agents/spawn", {
          agent: agentId,
          workspace_id: workspaceId,
          cwd,
          skip_permissions: skipPermissions,
          activate: true,
        });
        refreshFromBackend();
      } catch (e) {
        console.error(`Failed to spawn ${agentId} tab:`, e);
      }
      return;
    }
    const guardKey = `${workspaceId}:${agentId}`;
    if (spawnInFlight.current.has(guardKey)) return;
    spawnInFlight.current.add(guardKey);
    try {
      // One atomic backend command: create + cwd + agent_type + chat_view +
      // activate + focus, picking the transport (PTY vs chat) on the backend.
      // It returns the authoritative snapshot (applied once — no refetch, no
      // blink) and, for PTY, the launch command the terminal delivers.
      const res = await invoke<AtomicSpawn>("mux_spawn_pane", {
        params: { agentId, workspaceId, cwd, skipPermissions },
      });
      dispatch({ type: "SET_STATE", state: res.snapshot });
      if (res.launch_command) {
        if (cwd) setPendingCwd(res.pane_id, cwd);
        setPendingCommand(res.pane_id, res.launch_command);
      }
      // Snappy local focus; backend also emits the authoritative pane-focused.
      dispatch({ type: "SET_FOCUSED_PANE", paneId: res.pane_id });
    } catch (e) {
      const message = e instanceof Error ? e.message : typeof e === "string" ? e : String(e);
      const providerMissing = message.includes("Provider not installed:");
      dispatch({
        type: "ADD_NOTIFICATION",
        notification: {
          id: uid(),
          pane_id: null,
          workspace_id: workspaceId,
          title: providerMissing ? "Provider not installed" : `Failed to spawn ${agentId}`,
          body: message,
          level: "Error",
          timestamp: Date.now(),
          read: false,
          sticky: providerMissing,
        },
      });
      console.error(`Failed to spawn ${agentId} tab:`, e);
    } finally {
      spawnInFlight.current.delete(guardKey);
    }
  }, [refreshFromBackend]);

  const spawnClaudeTab = useCallback(async (workspaceId: string, skipPermissions: boolean, cwd?: string) => {
    spawnAgentTab(workspaceId, "claude", skipPermissions, cwd);
  }, [spawnAgentTab]);

  const closeTab = useCallback(
    (workspaceId: string, tabId: string) => {
      const { workspaces, panes } = stateRef.current;
      const ws = workspaces.find((w) => w.id === workspaceId);
      const tab = ws?.tabs.find((t) => t.id === tabId);
      if (!tab) return;
      const removedPaneIds = [tab.pane_id];
      for (const pid of removedPaneIds) {
        invoke("mux_close_pane", { paneId: pid }).catch(() => {});
      }
      const allRemoved = new Set(removedPaneIds);
      const collectChildren = (parentId: string) => {
        for (const [id, p] of Object.entries(panes)) {
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
    [refreshFromBackend],
  );

  const setActiveTab = useCallback((workspaceId: string, tabId: string) => {
    invoke("mux_set_active_tab_and_workspace", { workspaceId, tabId }).catch(() => {});
    dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
    dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId });
  }, []);

  const closePane = useCallback(
    (paneId: string) => {
      const ctx = findContext(paneId);
      if (!ctx) return;
      const { panes, workspaces } = stateRef.current;
      const pane = panes[paneId];
      if (pane?.pty_id) {
        invoke("kill_pty", { ptyId: pane.pty_id }).catch(() => {});
      }
      invoke("mux_close_pane", { paneId }).catch(() => {});
      const ws = workspaces.find((w) => w.id === ctx.workspaceId);
      const tab = ws?.tabs.find((t) => t.id === ctx.tabId);
      if (tab) {
        const allPanes = [tab.pane_id];
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
    [findContext],
  );

  const setFocusedPane = useCallback((paneId: string) => {
    // Optimistic local update for snappy click feedback.
    dispatch({ type: "SET_FOCUSED_PANE", paneId });
    // Backend re-emits `pane-focused` and keeps `focused_pane_id` authoritative.
    invoke("mux_set_focused_pane", { paneId }).catch(() => {});
  }, []);

  const setPanePty = useCallback((paneId: string, ptyId: string) => {
    dispatch({ type: "SET_PANE_PTY", paneId, ptyId });
  }, []);

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

  const upsertToast = useCallback(
    (toast: Omit<Notification, "timestamp" | "read">) => {
      dispatch({
        type: "UPSERT_TOAST",
        toast: {
          ...toast,
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

  const setTabTitle = useCallback((workspaceId: string, tabId: string, title: string) => {
    dispatch({ type: "SET_TAB_TITLE", workspaceId, tabId, title });
  }, []);

  const setAgentType = useCallback((paneId: string, agentType: string | null) => {
    dispatch({ type: "SET_AGENT_TYPE", paneId, agentType });
  }, []);

  const updatePaneStatus = useCallback((paneId: string, status: PaneStatus) => {
    dispatch({ type: "UPDATE_PANE_STATUS", paneId, status });
  }, []);

  const value = useMemo<MultiplexerContextType>(
    () => ({
      state,
      createWorkspace,
      setActiveWorkspace,
      closeWorkspace,
      renameWorkspace,
      moveTab,
      createTab,
      closeTab,
      reopenClosed,
      restoreOrphaned,
      setActiveTab,
      closePane,
      setFocusedPane,
      setPanePty,
      addNotification,
      upsertToast,
      markRead,
      markAllRead,
      clearNotifications,
      dismissToast,
      setPaneTitle,
      setTabTitle,
      setAgentType,
      updatePaneStatus,
      spawnClaudeTab,
      spawnAgentTab,
    }),
    [
      state,
      createWorkspace,
      setActiveWorkspace,
      closeWorkspace,
      renameWorkspace,
      moveTab,
      createTab,
      closeTab,
      reopenClosed,
      restoreOrphaned,
      setActiveTab,
      closePane,
      setFocusedPane,
      setPanePty,
      addNotification,
      upsertToast,
      markRead,
      markAllRead,
      clearNotifications,
      dismissToast,
      setPaneTitle,
      setTabTitle,
      setAgentType,
      updatePaneStatus,
      spawnClaudeTab,
      spawnAgentTab,
    ],
  );

  return (
    <MultiplexerContext.Provider value={value}>
      {children}
    </MultiplexerContext.Provider>
  );
}
