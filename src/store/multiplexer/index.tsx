import {
  useCallback,
  useMemo,
  useReducer,
  useEffect,
  useRef,
  type ReactNode,
} from "react";
import { invoke, reopenClosedPane, reopenClosedGroup, restoreOrphanedPane } from "../../lib/bridge";
import { listen } from "../../lib/bridge";
import { IS_WEB, webJson } from "../../lib/webApi";
import {
  loadCachedStateSync,
  loadCachedStateAsync,
  saveStateSnapshot,
  createWriteGate,
  type WriteGate,
} from "../../lib/stateCache";
import { mountWebLifecycle } from "../../lib/lifecycle";
import { uid } from "../../lib/uid";
import { setPendingCommand, setPendingCwd } from "../../lib/pendingCommands";
import { loadDefaultNewAgentPaneChat } from "../../lib/chatUiMode";
import { defaultSpawnProviderId } from "../../lib/agentDescriptorCache";
import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Notification,
  PaneStatus,
  AtomicSpawn,
  OrchestrationPairInput,
  OrchestrationPairResult,
} from "../../lib/types";
import {
  makeVirtualPickerTab,
  type ExtendedState,
} from "./tree";
import { reducer } from "./reducer";
import { MultiplexerContext, type MultiplexerContextType, type SpawnedPane } from "./context";

export { useMultiplexer } from "./context";
export { highestTitleLevel } from "./tree";

const HYDRATED_GENERATION = 0.5;

/** True when the web boot path paints sync cached mux state before reconcile. */
export function webBootHasSyncCache(): boolean {
  return IS_WEB && loadCachedStateSync() !== null;
}

function buildInitialState(): ExtendedState {
  const empty: ExtendedState = {
    workspaces: [],
    panes: {},
    active_workspace_id: null,
    focused_pane_id: null,
    notifications: [],
    paneStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
    virtualTabs: {},
  };
  if (!IS_WEB) return empty;
  const cached = loadCachedStateSync();
  if (!cached) return empty;
  return {
    ...empty,
    workspaces: cached.workspaces,
    panes: cached.panes,
    active_workspace_id: cached.active_workspace_id,
    focused_pane_id: cached.focused_pane_id,
    recently_closed: cached.recently_closed,
    generation: HYDRATED_GENERATION,
  };
}

export function muxStateEventAction(payload: unknown): "apply-snapshot" | "refresh" {
  if (
    payload &&
    typeof payload === "object" &&
    Array.isArray((payload as { workspaces?: unknown }).workspaces)
  ) {
    return "apply-snapshot";
  }
  return "refresh";
}

export function MultiplexerProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, null, buildInitialState);
  const stateRef = useRef(state);
  stateRef.current = state;
  // so one click = exactly one tab (no duplicate on double-fire).
  const spawnInFlight = useRef<Set<string>>(new Set());
  // authoritative reconcile lands; native boots never lock (no cache path).
  const writeGateRef = useRef<WriteGate>(createWriteGate(webBootHasSyncCache()));
  const reconciledRef = useRef(false);

  const applyBackendState = useCallback((s: MultiplexerState) => {
    // here silently dropped the closed-history on every reload.
    dispatch({ type: "SET_STATE", state: s });
    if (IS_WEB) saveStateSnapshot(s);
  }, []);

  // letting a cache-painted UI act on a target that may no longer exist.
  const guardedInvoke = useCallback(
    <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      if (!writeGateRef.current.canWrite()) {
        return Promise.reject(new Error(`Blocked "${cmd}": awaiting first daemon reconcile`));
      }
      return invoke<T>(cmd, args);
    },
    [],
  );

  const guardedWebJson = useCallback(
    <T,>(method: string, path: string, body?: unknown): Promise<T> => {
      if (!writeGateRef.current.canWrite()) {
        return Promise.reject(new Error(`Blocked "${path}": awaiting first daemon reconcile`));
      }
      return webJson<T>(method, path, body);
    },
    [],
  );

  // Load initial state from Rust backend (which already spawned PTY)
  useEffect(() => {
    const hydratedAtBoot = IS_WEB && stateRef.current.generation === HYDRATED_GENERATION;
    if (hydratedAtBoot) {
      writeGateRef.current.lock();
      window.dispatchEvent(new Event("codeterm-cache-hydrated"));
    } else if (IS_WEB) {
      loadCachedStateAsync().then((cached) => {
        if (!cached || reconciledRef.current) return;
        writeGateRef.current.lock();
        dispatch({ type: "SET_STATE", state: { ...cached, generation: HYDRATED_GENERATION } });
        window.dispatchEvent(new Event("codeterm-cache-hydrated"));
      });
    }

    invoke<MultiplexerState>("mux_get_state")
      .then((backendState) => {
        if (backendState && backendState.workspaces.length > 0) {
          applyBackendState(backendState);
        }
        reconciledRef.current = true;
        writeGateRef.current.unlock();
        if (IS_WEB) window.dispatchEvent(new Event("codeterm-cache-reconciled"));
      })
      .catch((err) => {
        console.warn("Failed to load backend state, using local state:", err);
      });
  }, [applyBackendState]);

  useEffect(() => mountWebLifecycle(), []);

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

    listen<MultiplexerState>("mux-state-changed", (event) => {
      const s = event.payload;
      if (muxStateEventAction(s) === "apply-snapshot") {
        applyBackendState(s);
        // Resize panes after layout changes so xterms fit new container sizes.
        requestAnimationFrame(() => {
          window.dispatchEvent(new Event("codeterm-pane-resize"));
        });
      } else {
        refreshFromBackend();
      }
    }).then((unlisten) => unlisteners.push(unlisten));

    const refreshMuxState = () => refreshFromBackend();
    window.addEventListener("codeterm-refresh-mux-state", refreshMuxState);
    unlisteners.push(() => window.removeEventListener("codeterm-refresh-mux-state", refreshMuxState));

    const onBootChanged = () => dispatch({ type: "RESET_GENERATION" });
    window.addEventListener("codeterm-web-boot-changed", onBootChanged);
    unlisteners.push(() => window.removeEventListener("codeterm-web-boot-changed", onBootChanged));

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
  }, [applyBackendState]);

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
          applyBackendState(s);
        }
      })
      .catch((e) => console.warn("Failed to refresh state:", e));
  }, [applyBackendState]);

  const createTab = useCallback(async (workspaceId: string, cwd?: string) => {
    try {
      guardedInvoke("mux_set_active_workspace", { workspaceId }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
      const tabInfo = await guardedInvoke<{ id: string; title: string; pane_id: string }>("mux_create_tab", { workspaceId, activate: true });
      const ws = stateRef.current.workspaces.find((w) => w.id === workspaceId);
      const effectiveCwd = cwd || ws?.cwd || undefined;
      if (effectiveCwd) {
        setPendingCwd(tabInfo.pane_id, effectiveCwd);
        await guardedInvoke("mux_update_pane_cwd", { paneId: tabInfo.pane_id, cwd: effectiveCwd });
      }
      await guardedInvoke("mux_set_active_tab", { workspaceId, tabId: tabInfo.id });
      dispatch({
        type: "SET_ACTIVE_TAB",
        workspaceId,
        tabId: tabInfo.id,
      });
      // Explicit New Tab click focuses the new pane; background spawns don't.
      guardedInvoke("mux_set_focused_pane", { paneId: tabInfo.pane_id }).catch(() => {});
      refreshFromBackend();
      return { id: tabInfo.id, pane_id: tabInfo.pane_id };
    } catch (e) {
      console.error("Failed to create tab:", e);
    }
  }, [refreshFromBackend, guardedInvoke]);

  const openSessionPicker = useCallback((workspaceId: string, cwd?: string) => {
    guardedInvoke("mux_set_active_workspace", { workspaceId }).catch(() => {});
    const ws = stateRef.current.workspaces.find((w) => w.id === workspaceId);
    const effectiveCwd = cwd || ws?.cwd || undefined;
    dispatch({
      type: "OPEN_SESSION_PICKER",
      workspaceId,
      virtual: makeVirtualPickerTab(effectiveCwd),
    });
  }, [guardedInvoke]);

  const closeSessionPicker = useCallback((workspaceId: string, tabId: string) => {
    dispatch({ type: "CLOSE_SESSION_PICKER", workspaceId, tabId });
  }, []);

  const reopenClosed = useCallback(async (workspaceId: string, index = 0) => {
    try {
      guardedInvoke("mux_set_active_workspace", { workspaceId }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
      const tabInfo = await reopenClosedPane(workspaceId, index);
      await guardedInvoke("mux_set_active_tab", { workspaceId, tabId: tabInfo.id }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId: tabInfo.id });
      guardedInvoke("mux_set_focused_pane", { paneId: tabInfo.pane_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to reopen closed pane:", e);
    }
  }, [refreshFromBackend, guardedInvoke]);

  const reopenGroup = useCallback(async (groupId: string) => {
    try {
      const result = await reopenClosedGroup(groupId);
      guardedInvoke("mux_set_active_workspace", { workspaceId: result.workspace_id }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: result.workspace_id });
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId: result.workspace_id, tabId: result.tab_id });
      guardedInvoke("mux_set_focused_pane", { paneId: result.pane_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to reopen closed group:", e);
    }
  }, [refreshFromBackend, guardedInvoke]);

  const restoreOrphaned = useCallback(async (paneId: string) => {
    try {
      const result = await restoreOrphanedPane(paneId);
      guardedInvoke("mux_set_active_workspace", { workspaceId: result.workspace_id }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: result.workspace_id });
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId: result.workspace_id, tabId: result.tab_id });
      guardedInvoke("mux_set_focused_pane", { paneId: result.pane_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to restore orphaned pane:", e);
    }
  }, [refreshFromBackend, guardedInvoke]);

  const createWorkspace = useCallback(async (name: string, cwd?: string) => {
    try {
      // Reuse existing workspace for the same cwd; avoid duplicate tabs.
      const info = await guardedInvoke<{ id: string; name: string }>("mux_create_workspace", { name, cwd: cwd ?? null, activate: true });
      const s = await invoke<MultiplexerState>("mux_get_state");
      if (s && s.workspaces.length > 0) applyBackendState(s);
      return info;
    } catch (e) {
      console.error("Failed to create workspace:", e);
      return undefined;
    }
  }, [guardedInvoke, applyBackendState]);

  const setActiveWorkspace = useCallback((id: string) => {
    guardedInvoke("mux_set_active_workspace", { workspaceId: id }).catch(() => {});
    dispatch({ type: "SET_ACTIVE_WORKSPACE", id });
  }, [guardedInvoke]);

  const closeWorkspace = useCallback(
    (id: string) => {
      const { workspaces, panes } = stateRef.current;
      const ws = workspaces.find((w) => w.id === id);
      if (ws) {
        const paneIds = ws.tabs.map((t) => t.pane_id);
        for (const pid of paneIds) {
          const pane = panes[pid];
          if (pane?.pty_id) {
            guardedInvoke("kill_pty", { ptyId: pane.pty_id }).catch(() => {});
          }
        }
      }
      guardedInvoke("mux_close_workspace", { workspaceId: id }).catch(() => {});
      dispatch({ type: "CLOSE_WORKSPACE", id });
    },
    [guardedInvoke],
  );

  const renameWorkspace = useCallback((id: string, name: string) => {
    guardedInvoke("mux_rename_workspace", { workspaceId: id, name }).catch(() => {});
    dispatch({ type: "RENAME_WORKSPACE", id, name });
  }, [guardedInvoke]);

  const moveTab = useCallback((tabId: string, fromWorkspaceId: string, toWorkspaceId: string) => {
    guardedInvoke("mux_move_tab", { tabId, fromWorkspaceId, toWorkspaceId }).catch(() => {});
    dispatch({ type: "MOVE_TAB", tabId, fromWorkspaceId, toWorkspaceId });
  }, [guardedInvoke]);

  const reorderTabs = useCallback(
    (args: { workspaceId?: string; parentPaneId?: string; orderedTabIds: string[] }) => {
      // authoritative SET_STATE (mux-state-changed) arrives.
      dispatch({ type: "REORDER_TABS", orderedTabIds: args.orderedTabIds });
      if (args.parentPaneId) {
        guardedInvoke("mux_reorder_child", {
          parentPaneId: args.parentPaneId,
          orderedTabIds: args.orderedTabIds,
        }).catch(() => {});
      } else if (args.workspaceId) {
        guardedInvoke("mux_reorder_tab", {
          workspaceId: args.workspaceId,
          orderedTabIds: args.orderedTabIds,
        }).catch(() => {});
      }
    },
    [guardedInvoke],
  );

  const reorderWorkspaces = useCallback((orderedWorkspaceIds: string[]) => {
    dispatch({ type: "REORDER_WORKSPACES", orderedWorkspaceIds });
    guardedInvoke("mux_reorder_workspace", { orderedWorkspaceIds }).catch(() => {});
  }, [guardedInvoke]);

  const notifySpawnFailure = useCallback((workspaceId: string, agentId: string, e: unknown) => {
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
  }, []);

  // Never throws — self-notifies on failure and returns null so callers can
  // branch (the picker keeps itself open on null). Success returns the ids the
  // caller needs to select/focus the freshly-created tab.
  const spawnAgentTab = useCallback(async (
    workspaceId: string,
    agentId: string,
    skipPermissions: boolean,
    cwd?: string,
    model?: string,
  ): Promise<SpawnedPane | null> => {
    const useRestSpawn = IS_WEB;
    if (useRestSpawn) {
      try {
        const res = await guardedWebJson<{ pane_id: string }>("POST", "/api/agents/spawn", {
          agent: agentId,
          workspace_id: workspaceId,
          cwd,
          skip_permissions: skipPermissions,
          model,
          activate: true,
        });
        refreshFromBackend();
        return { workspaceId, paneId: res.pane_id };
      } catch (e) {
        notifySpawnFailure(workspaceId, agentId, e);
        return null;
      }
    }
    const guardKey = `${workspaceId}:${agentId}`;
    if (spawnInFlight.current.has(guardKey)) return null;
    spawnInFlight.current.add(guardKey);
    try {
      // activate + focus, picking the transport (PTY vs chat) on the backend.
      const res = await guardedInvoke<AtomicSpawn>("mux_spawn_pane", {
        params: { agentId, workspaceId, cwd, skipPermissions, model },
      });
      applyBackendState(res.snapshot);
      if (res.launch_command) {
        if (cwd) setPendingCwd(res.pane_id, cwd);
        setPendingCommand(res.pane_id, res.launch_command);
      }
      // Snappy local focus; backend also emits the authoritative pane-focused.
      dispatch({ type: "SET_FOCUSED_PANE", paneId: res.pane_id });
      return { workspaceId: res.workspace_id, paneId: res.pane_id };
    } catch (e) {
      notifySpawnFailure(workspaceId, agentId, e);
      return null;
    } finally {
      spawnInFlight.current.delete(guardKey);
    }
  }, [refreshFromBackend, guardedInvoke, guardedWebJson, applyBackendState, notifySpawnFailure]);

  // from the descriptor registry rather than hardcoding a vendor id.
  const spawnClaudeTab = useCallback(async (workspaceId: string, skipPermissions: boolean, cwd?: string) => {
    spawnAgentTab(workspaceId, defaultSpawnProviderId(), skipPermissions, cwd);
  }, [spawnAgentTab]);

  const createOrchestrationPair = useCallback(
    async (input: OrchestrationPairInput): Promise<OrchestrationPairResult> => {
      const body = {
        orchestrator_agent: input.orchestratorAgent,
        orchestrator_model: input.orchestratorModel,
        planner_agent: input.plannerAgent,
        planner_model: input.plannerModel,
        workspace_id: input.workspaceId,
        title: input.title,
        existing_tab_id: input.existingTabId,
      };
      const result = IS_WEB
        ? await guardedWebJson<OrchestrationPairResult>("POST", "/api/orchestration/new", body)
        : await guardedInvoke<OrchestrationPairResult>("mux_create_orchestration_pair", { params: body });
      refreshFromBackend();
      return result;
    },
    [guardedInvoke, guardedWebJson, refreshFromBackend],
  );

  const closeTab = useCallback(
    (workspaceId: string, tabId: string) => {
      const { workspaces, panes } = stateRef.current;
      const ws = workspaces.find((w) => w.id === workspaceId);
      const tab = ws?.tabs.find((t) => t.id === tabId);
      if (!tab) return;
      // send a close to the daemon for a pane it never knew about.
      if (stateRef.current.virtualTabs[workspaceId]?.tab.id === tabId) {
        dispatch({ type: "CLOSE_SESSION_PICKER", workspaceId, tabId });
        return;
      }
      const removedPaneIds = [tab.pane_id];
      for (const pid of removedPaneIds) {
        guardedInvoke("mux_close_pane", { paneId: pid }).catch(() => {});
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
    [refreshFromBackend, guardedInvoke],
  );

  const setActiveTab = useCallback((workspaceId: string, tabId: string) => {
    guardedInvoke("mux_set_active_tab_and_workspace", { workspaceId, tabId }).catch(() => {});
    dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
    dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId });
  }, [guardedInvoke]);

  // Select + focus a just-spawned pane by resolving its owning tab. A freshly
  // created pane may not be in local state yet (spawn refresh is async), so
  // fall back to an authoritative snapshot before giving up.
  const selectTabByPane = useCallback(async (workspaceId: string, paneId: string) => {
    const findTab = (s: MultiplexerState) =>
      s.workspaces.find((w) => w.id === workspaceId)?.tabs.find((t) => t.pane_id === paneId)?.id ?? null;
    let tabId = findTab(stateRef.current);
    if (!tabId) {
      try {
        const s = await invoke<MultiplexerState>("mux_get_state");
        if (s && s.workspaces.length > 0) {
          applyBackendState(s);
          tabId = findTab(s);
        }
      } catch (e) {
        console.warn("Failed to resolve spawned tab:", e);
      }
    }
    if (!tabId) return;
    guardedInvoke("mux_set_active_tab_and_workspace", { workspaceId, tabId }).catch(() => {});
    dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
    dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId });
    guardedInvoke("mux_set_focused_pane", { paneId }).catch(() => {});
    dispatch({ type: "SET_FOCUSED_PANE", paneId });
  }, [guardedInvoke, applyBackendState]);

  const closePane = useCallback(
    (paneId: string) => {
      const ctx = findContext(paneId);
      if (!ctx) return;
      const { panes, workspaces } = stateRef.current;
      const pane = panes[paneId];
      if (pane?.pty_id) {
        guardedInvoke("kill_pty", { ptyId: pane.pty_id }).catch(() => {});
      }
      guardedInvoke("mux_close_pane", { paneId }).catch(() => {});
      const ws = workspaces.find((w) => w.id === ctx.workspaceId);
      const tab = ws?.tabs.find((t) => t.id === ctx.tabId);
      if (tab) {
        const allPanes = [tab.pane_id];
        if (allPanes.length <= 1) {
          guardedInvoke("mux_close_tab", { workspaceId: ctx.workspaceId, tabId: ctx.tabId }).catch(() => {});
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
    [findContext, guardedInvoke],
  );

  const setFocusedPane = useCallback((paneId: string) => {
    // Optimistic local update for snappy click feedback.
    dispatch({ type: "SET_FOCUSED_PANE", paneId });
    // Backend re-emits `pane-focused` and keeps `focused_pane_id` authoritative.
    guardedInvoke("mux_set_focused_pane", { paneId }).catch(() => {});
  }, [guardedInvoke]);

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
    const workspace = stateRef.current.workspaces.find((ws) => ws.id === workspaceId);
    const tab = workspace?.tabs.find((candidate) => candidate.id === tabId);
    const paneId = tab?.pane_id;
    if (!paneId) return;
    dispatch({ type: "SET_PANE_TITLE", paneId, title, level: TitleLevel.User });
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
      reorderTabs,
      reorderWorkspaces,
      createTab,
      openSessionPicker,
      closeSessionPicker,
      closeTab,
      reopenClosed,
      reopenGroup,
      restoreOrphaned,
      setActiveTab,
      selectTabByPane,
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
      createOrchestrationPair,
    }),
    [
      state,
      createWorkspace,
      setActiveWorkspace,
      closeWorkspace,
      renameWorkspace,
      moveTab,
      reorderTabs,
      reorderWorkspaces,
      createTab,
      openSessionPicker,
      closeSessionPicker,
      closeTab,
      reopenClosed,
      reopenGroup,
      restoreOrphaned,
      setActiveTab,
      selectTabByPane,
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
      createOrchestrationPair,
    ],
  );

  return (
    <MultiplexerContext.Provider value={value}>
      {children}
    </MultiplexerContext.Provider>
  );
}
