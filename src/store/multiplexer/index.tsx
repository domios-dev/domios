import {
  useCallback,
  useMemo,
  useReducer,
  useEffect,
  useRef,
  type ReactNode,
} from "react";
import { invoke, reopenClosedTab, reopenClosedGroup, restoreOrphanedTab } from "../../lib/bridge";
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
import { defaultSpawnProviderId } from "../../lib/agentDescriptorCache";
import { TitleLevel } from "../../lib/types";
import { adoptStoredActions } from "../../lib/notificationActions";
import type {
  MultiplexerState,
  Notification,
  NotificationAnswer,
  TabStatus,
  AtomicSpawn,
  OrchestrationPairInput,
  OrchestrationPairResult,
  TabBootStageEvent,
} from "../../lib/types";
import type { ExtendedState } from "./tree";
import { reducer, type Action } from "./reducer";
import { applyPendingTabGuard, type TabGuards } from "./pendingTabGuard";
import { mountMuxPerfTestHook } from "./muxPerfTestHook";
import { isCodeTermFrontendTestMode } from "../../components/Terminal/useChatSession";
import { MultiplexerActionsContext, MuxStateStoreContext, type MultiplexerActionsContextType, type SpawnedTab } from "./context";
import { createMuxStateStore } from "./selectorStore";
import type { Tab } from "../../lib/types";
import { remoteTabsFromOrdinaryProjection } from "./remoteTabProjection";
import {
  loadAttachedRemoteTabs,
  projectAttachedRemoteTab,
  saveAttachedRemoteTabs,
  type RemotePeerTabs,
} from "../../lib/remoteTabPortal";
import { onRemoteTransportState } from "../../lib/remoteTabTransportState";
import { resumeRemoteOutputStreams } from "../../lib/outputStream";
import { resumeRemoteTabEvents } from "../../lib/remoteTabEvents";

export { useMultiplexer } from "./context";
export { useMuxState, useMuxSelector } from "./selectorStore";
export { highestTitleLevel } from "./tree";

const HYDRATED_GENERATION = 0.5;
const MUX_EVENT_RENEW_MS = 10_000;

/** True when the web boot path paints sync cached mux state before reconcile. */
export function webBootHasSyncCache(): boolean {
  return IS_WEB && loadCachedStateSync() !== null;
}

function buildInitialState(): ExtendedState {
  const empty: ExtendedState = {
    workspaces: [],
    tabs: {},
    teams: [],
    active_workspace_id: null,
    focused_tab_id: null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
    pendingCloses: [],
  };
  if (!IS_WEB) return empty;
  const cached = loadCachedStateSync();
  if (!cached) return empty;
  return {
    ...empty,
    workspaces: cached.workspaces,
    tabs: cached.tabs,
    teams: cached.teams ?? [],
    active_workspace_id: cached.active_workspace_id,
    focused_tab_id: cached.focused_tab_id,
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

export function tabBootStageEventAction(payload: TabBootStageEvent): Action {
  return {
    type: "UPDATE_BOOT_STAGE",
    tabId: payload.tab_id,
    stage: payload.stage,
    reason: payload.reason,
  };
}

export function MultiplexerProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, null, buildInitialState);
  const stateRef = useRef(state);
  stateRef.current = state;
  const muxStateStoreRef = useRef(createMuxStateStore(() => stateRef.current));
  // so one click = exactly one tab (no duplicate on double-fire).
  const spawnInFlight = useRef<Set<string>>(new Set());
  // authoritative reconcile lands; native boots never lock (no cache path).
  const writeGateRef = useRef<WriteGate>(createWriteGate(webBootHasSyncCache()));
  const reconciledRef = useRef(false);
  // In-flight user tab selections; masks stale snapshots until the backend
  // confirms the switch, rejects it, or removes the target tab.
  const pendingTabSwitchRef = useRef<TabGuards>({});
  const peerSwitchRef = useRef(0);
  const attachedRemoteTabsRef = useRef<Record<string, Tab>>(loadAttachedRemoteTabs());

  const applyRemoteProjection = useCallback((tabs: Record<string, Tab>) => {
    dispatch({ type: "SET_REMOTE_TABS", tabs: { ...tabs, ...attachedRemoteTabsRef.current } });
  }, []);

  const applyBackendState = useCallback((s: MultiplexerState) => {
    // here silently dropped the closed-history on every reload.
    const guarded = applyPendingTabGuard(s, pendingTabSwitchRef.current);
    dispatch({ type: "SET_STATE", state: guarded });
    const remoteTabs = Object.fromEntries(
      Object.entries(stateRef.current.tabs).filter(([, tab]) => Boolean(tab.remote_peer_id)),
    );
    if (Object.keys(remoteTabs).length > 0) {
      applyRemoteProjection(remoteTabs);
    }
    if (IS_WEB) saveStateSnapshot(s);
  }, [applyRemoteProjection]);

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
        if (backendState) applyBackendState(backendState);
        reconciledRef.current = true;
        writeGateRef.current.unlock();
        if (IS_WEB) window.dispatchEvent(new Event("codeterm-cache-reconciled"));
      })
      .catch((err) => {
        console.warn("Failed to load backend state, using local state:", err);
      });
    invoke<Notification[]>("list_notifications", { unreadOnly: false })
      .then((notifications) =>
        dispatch({
          type: "HYDRATE_NOTIFICATIONS",
          // Ingress for stored rows: an action written before actions carried
          // daemon provenance loads with its invoke stripped, never accepted.
          notifications: notifications.map((notification) => ({
            ...notification,
            actions: adoptStoredActions(notification.actions),
          })),
        }),
      )
      .catch((err) => console.warn("Failed to load notification history:", err));
  }, [applyBackendState]);

  useEffect(() => mountWebLifecycle(), []);

  const refreshRemoteProjection = useCallback(async () => {
    const ordinary = await invoke<Tab[]>("ordinary_tabs_list");
    const remote = remoteTabsFromOrdinaryProjection(ordinary);
    applyRemoteProjection(remote);
    resumeRemoteOutputStreams();
    resumeRemoteTabEvents();
  }, [applyRemoteProjection]);

  useEffect(() => {
    let active = true;
    const refreshRemoteTabs = async () => {
      const ordinary = await invoke<Tab[]>("ordinary_tabs_list");
      if (!active) return;
      const remote = remoteTabsFromOrdinaryProjection(ordinary);
      applyRemoteProjection(remote);
      resumeRemoteOutputStreams();
      resumeRemoteTabEvents();
    };
    void refreshRemoteTabs().catch(() => undefined);
    let unlisten: (() => void) | undefined;
    void listen("mesh-state-changed", () => {
      for (const tab of Object.values(stateRef.current.tabs)) {
        if (!tab.remote_peer_id) continue;
        for (const channel of ["control", "output", "chat"] as const) {
          dispatch({ type: "UPDATE_REMOTE_CHANNEL", tabId: tab.id, channel, availability: "reconnecting" });
        }
      }
      void refreshRemoteTabs();
    }).then((fn) => { unlisten = fn; });
    return () => { active = false; unlisten?.(); };
  }, [applyRemoteProjection]);

  const attachRemoteTab = useCallback((workspaceId: string, peer: RemotePeerTabs, tab: Tab) => {
    const projected = projectAttachedRemoteTab(tab, peer, workspaceId);
    attachedRemoteTabsRef.current = { ...attachedRemoteTabsRef.current, [projected.id]: projected };
    saveAttachedRemoteTabs(attachedRemoteTabsRef.current);
    applyRemoteProjection(remoteTabsFromOrdinaryProjection(Object.values(stateRef.current.tabs)));
    dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
    dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId: projected.id });
    dispatch({ type: "SET_FOCUSED_TAB", tabId: projected.id });
  }, [applyRemoteProjection]);

  useEffect(() => onRemoteTransportState(({ ordinaryTabId, channel, availability }) => {
    dispatch({ type: "UPDATE_REMOTE_CHANNEL", tabId: ordinaryTabId, channel, availability });
  }), []);

  useEffect(() => {
    const unlisteners: (() => void)[] = [];

    listen<{
      id: string;
      tab_id: string | null;
      workspace_id: string | null;
      title: string;
      body: string | null;
      severity: "Info" | "Warning" | "Error";
      level?: "verbose" | "action_needed" | "failure";
      actions?: unknown;
      answer?: NotificationAnswer;
      sticky?: boolean;
    }>("notification-created", (event) => {
      dispatch({
        type: "ADD_NOTIFICATION",
        notification: {
          id: event.payload.id,
          tab_id: event.payload.tab_id,
          workspace_id: event.payload.workspace_id,
          title: event.payload.title,
          body: event.payload.body,
          severity: event.payload.severity,
          level: event.payload.level,
          timestamp: Date.now(),
          read: false,
          actions: adoptStoredActions(event.payload.actions),
          answer: event.payload.answer,
          sticky: event.payload.sticky,
        },
      });
    }).then((unlisten) => unlisteners.push(unlisten));

    listen<{ id: string; answered: string; delivered?: boolean }>(
      "notification-answered",
      (event) => {
        if (!event.payload?.id) return;
        dispatch({
          type: "ANSWER_NOTIFICATION",
          id: event.payload.id,
          answered: event.payload.answered,
          delivered: event.payload.delivered === true,
        });
      },
    ).then((unlisten) => unlisteners.push(unlisten));

    listen<{ id: string }>("notification-resolved", (event) => {
      if (!event.payload?.id) return;
      dispatch({ type: "MARK_READ", id: event.payload.id });
    }).then((unlisten) => unlisteners.push(unlisten));

    listen<{ id: string }>("notification-cleared", (event) => {
      if (!event.payload?.id) return;
      dispatch({ type: "DELETE_NOTIFICATION", id: event.payload.id });
      dispatch({ type: "DISMISS_TOAST", id: event.payload.id });
    }).then((unlisten) => unlisteners.push(unlisten));

    listen<{ tab_id: string; status: TabStatus }>(
      "tab-status-changed",
      (event) => {
        if (!event.payload) return;
        dispatch({
          type: "UPDATE_TAB_STATUS",
          tabId: event.payload.tab_id,
          status: event.payload.status,
        });
      },
    ).then((unlisten) => unlisteners.push(unlisten));

    listen<TabBootStageEvent>("tab-boot-stage", (event) => {
      if (!event.payload) return;
      dispatch(tabBootStageEventAction(event.payload));
    }).then((unlisten) => unlisteners.push(unlisten));

    listen<{ tab_id: string; title: string }>("tab-title-pinned", (event) => {
      dispatch({
        type: "SET_TAB_TITLE",
        tabId: event.payload.tab_id,
        title: event.payload.title,
        level: TitleLevel.User,
      });
    }).then((unlisten) => unlisteners.push(unlisten));

    let muxUnlisten: (() => void) | null = null;
    let muxListenGeneration = 0;
    const eventStats = window as unknown as {
      __ctEvents?: { received: number; lastAt: number; reconnects: number; breakMuxStream?: () => void };
    };
    const applyMuxEvent = (event: { payload: MultiplexerState }) => {
      (eventStats.__ctEvents ??= { received: 0, lastAt: 0, reconnects: 0 }).received++;
      eventStats.__ctEvents.lastAt = Date.now();
      const s = event.payload;
      if (muxStateEventAction(s) === "apply-snapshot") {
        applyBackendState(s);
        // Resize tabs after layout changes so xterms fit new container sizes.
        requestAnimationFrame(() => {
          window.dispatchEvent(new Event("codeterm-tab-resize"));
        });
      } else {
        refreshFromBackend();
      }
    };
    const renewMuxListener = () => {
      const generation = ++muxListenGeneration;
      muxUnlisten?.();
      muxUnlisten = null;
      void listen<MultiplexerState>("mux-state-changed", applyMuxEvent)
        .then((unlisten) => {
          if (generation !== muxListenGeneration) {
            unlisten();
            return;
          }
          muxUnlisten = unlisten;
          (eventStats.__ctEvents ??= { received: 0, lastAt: 0, reconnects: 0 }).reconnects++;
        })
        .catch((error) => console.warn("Failed to renew mux event listener:", error));
      refreshFromBackend();
    };
    renewMuxListener();
    let muxRenewInterval = window.setInterval(renewMuxListener, MUX_EVENT_RENEW_MS);
    if (isCodeTermFrontendTestMode()) {
      (eventStats.__ctEvents ??= { received: 0, lastAt: 0, reconnects: 0 }).breakMuxStream = () => {
        muxUnlisten?.();
        muxUnlisten = null;
        window.clearInterval(muxRenewInterval);
        muxRenewInterval = window.setInterval(renewMuxListener, MUX_EVENT_RENEW_MS);
      };
    }
    unlisteners.push(() => {
      window.clearInterval(muxRenewInterval);
      muxListenGeneration++;
      muxUnlisten?.();
      delete eventStats.__ctEvents?.breakMuxStream;
    });

    const refreshMuxState = () => void refreshFromBackend();
    window.addEventListener("codeterm-refresh-mux-state", refreshMuxState);
    unlisteners.push(() => window.removeEventListener("codeterm-refresh-mux-state", refreshMuxState));

    const resetPeerContext = (event: Event) => {
      const switchId = (event as CustomEvent<{ switchId: number }>).detail.switchId;
      peerSwitchRef.current = switchId;
      pendingTabSwitchRef.current = {};
      writeGateRef.current.lock();
      dispatch({ type: "RESET_PEER_CONTEXT" });
    };
    const rebindPeerContext = (event: Event) => {
      const switchId = (event as CustomEvent<{ switchId: number }>).detail.switchId;
      void refreshFromBackend().then((success) => {
        if (peerSwitchRef.current !== switchId) return;
        if (success) writeGateRef.current.unlock();
        window.dispatchEvent(new CustomEvent("codeterm-peer-switch-ready", {
          detail: { switchId, success },
        }));
      });
    };
    window.addEventListener("codeterm-peer-switch-reset", resetPeerContext);
    window.addEventListener("codeterm-peer-switch-rebind", rebindPeerContext);
    unlisteners.push(() => window.removeEventListener("codeterm-peer-switch-reset", resetPeerContext));
    unlisteners.push(() => window.removeEventListener("codeterm-peer-switch-rebind", rebindPeerContext));

    const onBootChanged = () => dispatch({ type: "RESET_GENERATION" });
    window.addEventListener("codeterm-web-boot-changed", onBootChanged);
    unlisteners.push(() => window.removeEventListener("codeterm-web-boot-changed", onBootChanged));

    // Backend owns `focused_tab_id`; background events don't emit `tab-focused`.
    // Focus updates come only from CLI/REST or service fallback when the tab closes.
    listen<{ tab_id: string }>("tab-focused", (event) => {
      const tabId = event.payload.tab_id;
      if (stateRef.current.tabs[tabId]) {
        dispatch({ type: "SET_FOCUSED_TAB", tabId });
      } else {
        // Focus may arrive before full state merge; retry once after a short delay.
        setTimeout(() => {
          if (stateRef.current.tabs[tabId]) {
            dispatch({ type: "SET_FOCUSED_TAB", tabId });
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

  const refreshFromBackend = useCallback(async (): Promise<boolean> => {
    try {
      const s = await invoke<MultiplexerState>("mux_get_state");
      if (s) applyBackendState(s);
      return !!s;
    } catch (e) {
      if (!(e instanceof DOMException && e.name === "AbortError")) {
        console.warn("Failed to refresh state:", e);
      }
      return false;
    }
  }, [applyBackendState]);

  const createTab = useCallback(async (workspaceId: string, cwd?: string) => {
    try {
      guardedInvoke("mux_set_active_workspace", { workspaceId }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
      const tabInfo = await guardedInvoke<{ id: string; title: string }>("mux_create_tab", { workspaceId, activate: true });
      const ws = stateRef.current.workspaces.find((w) => w.id === workspaceId);
      const effectiveCwd = cwd || ws?.cwd || undefined;
      if (effectiveCwd) {
        setPendingCwd(tabInfo.id, effectiveCwd);
        await guardedInvoke("mux_update_tab_cwd", { tabId: tabInfo.id, cwd: effectiveCwd });
      }
      await guardedInvoke("mux_set_active_tab", { workspaceId, tabId: tabInfo.id });
      dispatch({
        type: "SET_ACTIVE_TAB",
        workspaceId,
        tabId: tabInfo.id,
      });
      // Explicit New Tab click focuses the new tab; background spawns don't.
      guardedInvoke("mux_set_focused_tab", { tabId: tabInfo.id }).catch(() => {});
      refreshFromBackend();
      return { id: tabInfo.id };
    } catch (e) {
      console.error("Failed to create tab:", e);
    }
  }, [refreshFromBackend, guardedInvoke]);

  const reopenClosed = useCallback(async (workspaceId: string, index = 0) => {
    try {
      guardedInvoke("mux_set_active_workspace", { workspaceId }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
      const tabInfo = await reopenClosedTab(workspaceId, index);
      dispatch({ type: "REMOVE_PENDING_CLOSE", tabIds: [tabInfo.tab_id] });
      await guardedInvoke("mux_set_active_tab", { workspaceId, tabId: tabInfo.id }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId: tabInfo.id });
      guardedInvoke("mux_set_focused_tab", { tabId: tabInfo.tab_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to reopen closed tab:", e);
    }
  }, [refreshFromBackend, guardedInvoke]);

  const reopenGroup = useCallback(async (groupId: string) => {
    try {
      const result = await reopenClosedGroup(groupId);
      dispatch({ type: "REMOVE_PENDING_CLOSE", tabIds: [result.tab_id] });
      guardedInvoke("mux_set_active_workspace", { workspaceId: result.workspace_id }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: result.workspace_id });
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId: result.workspace_id, tabId: result.tab_id });
      guardedInvoke("mux_set_focused_tab", { tabId: result.tab_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to reopen closed group:", e);
    }
  }, [refreshFromBackend, guardedInvoke]);

  const restoreOrphaned = useCallback(async (tabId: string) => {
    try {
      const result = await restoreOrphanedTab(tabId);
      dispatch({ type: "REMOVE_PENDING_CLOSE", tabIds: [tabId, result.tab_id] });
      guardedInvoke("mux_set_active_workspace", { workspaceId: result.workspace_id }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: result.workspace_id });
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId: result.workspace_id, tabId: result.tab_id });
      guardedInvoke("mux_set_focused_tab", { tabId: result.tab_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to restore orphaned tab:", e);
    }
  }, [refreshFromBackend, guardedInvoke]);

  const createWorkspace = useCallback(async (name: string, cwd?: string, opts?: { activate?: boolean }) => {
    try {
      // Reuse existing workspace for the same cwd; avoid duplicate tabs.
      const info = await guardedInvoke<{ id: string; name: string }>("mux_create_workspace", { name, cwd: cwd ?? null, activate: opts?.activate ?? true });
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
      const ws = stateRef.current.workspaces.find((w) => w.id === id);
      const tabIds = ws ? [...ws.tabs] : [];
      dispatch({ type: "CLOSE_WORKSPACE", id });
      // On rejection the workspace still lives — drop its tombstones and restore.
      guardedInvoke("mux_close_workspace", { workspaceId: id })
        .then(() => refreshFromBackend())
        .catch(() => {
          dispatch({ type: "REMOVE_PENDING_CLOSE", tabIds });
          refreshFromBackend();
        });
    },
    [guardedInvoke, refreshFromBackend],
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
    (args: { workspaceId?: string; parentTabId?: string; orderedTabIds: string[] }) => {
      // authoritative SET_STATE (mux-state-changed) arrives.
      dispatch({ type: "REORDER_TABS", orderedTabIds: args.orderedTabIds });
      if (args.parentTabId) {
        guardedInvoke("mux_reorder_child", {
          parentTabId: args.parentTabId,
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
      type: "UPSERT_TOAST",
      toast: {
        id: uid(),
        tab_id: null,
        workspace_id: workspaceId,
        title: providerMissing ? "Provider not installed" : `Failed to spawn ${agentId}`,
        body: message,
        severity: "Error",
        timestamp: Date.now(),
        read: false,
        sticky: providerMissing,
      },
    });
    console.error(`Failed to spawn ${agentId} tab:`, e);
  }, []);

  // Failures self-notify; success returns the ids needed to focus the new tab.
  const spawnAgentTab = useCallback(async (
    workspaceId: string,
    agentId: string,
    cwd?: string,
    model?: string,
  ): Promise<SpawnedTab | null> => {
    const useRestSpawn = IS_WEB;
    if (useRestSpawn) {
      try {
        const res = await guardedWebJson<{ tab_id: string }>("POST", "/api/agents/spawn", {
          agent: agentId,
          workspace_id: workspaceId,
          cwd,
          model,
          activate: true,
        });
        await refreshFromBackend();
        return { workspaceId, tabId: res.tab_id };
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
      const res = await guardedInvoke<AtomicSpawn>("mux_spawn_tab", {
        params: { agentId, workspaceId, cwd, model },
      });
      applyBackendState(res.snapshot);
      if (res.launch_command) {
        if (cwd) setPendingCwd(res.tab_id, cwd);
        setPendingCommand(res.tab_id, res.launch_command);
      }
      pendingTabSwitchRef.current[res.workspace_id] = { tabId: res.tab_id };
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: res.workspace_id });
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId: res.workspace_id, tabId: res.tab_id });
      // Snappy local focus; backend also emits the authoritative tab-focused.
      dispatch({ type: "SET_FOCUSED_TAB", tabId: res.tab_id });
      return { workspaceId: res.workspace_id, tabId: res.tab_id };
    } catch (e) {
      notifySpawnFailure(workspaceId, agentId, e);
      return null;
    } finally {
      spawnInFlight.current.delete(guardKey);
    }
  }, [refreshFromBackend, guardedInvoke, guardedWebJson, applyBackendState, notifySpawnFailure]);

  // from the descriptor registry rather than hardcoding a vendor id.
  const spawnClaudeTab = useCallback(async (workspaceId: string, cwd?: string) => {
    spawnAgentTab(workspaceId, defaultSpawnProviderId(), cwd);
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
      const { workspaces } = stateRef.current;
      const ws = workspaces.find((w) => w.id === workspaceId);
      if (!ws?.tabs.includes(tabId)) return;
      const remote = stateRef.current.tabs[tabId];
      const isRemote = Boolean(remote?.remote_peer_id);
      dispatch({ type: "CLOSE_TAB", workspaceId, tabId, removedTabIds: [tabId] });
      // Refresh only after the close is acknowledged; on rejection the tab
      // still lives, so drop its tombstone and restore from authoritative state.
      guardedInvoke("mux_close_tab", { workspaceId, tabId })
        .then(() => {
          if (isRemote) {
            if (attachedRemoteTabsRef.current[tabId]) {
              const { [tabId]: _, ...remaining } = attachedRemoteTabsRef.current;
              attachedRemoteTabsRef.current = remaining;
              saveAttachedRemoteTabs(remaining);
            }
            void refreshRemoteProjection();
          }
          else refreshFromBackend();
        })
        .catch(() => {
          dispatch({ type: "REMOVE_PENDING_CLOSE", tabIds: [tabId] });
          if (isRemote) void refreshRemoteProjection();
          else refreshFromBackend();
        });
    },
    [refreshFromBackend, refreshRemoteProjection, guardedInvoke],
  );

  const setActiveTab = useCallback((workspaceId: string, tabId: string) => {
    const remote = stateRef.current.tabs[tabId];
    if (remote?.remote_peer_id) {
      dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
      dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId });
      dispatch({ type: "SET_FOCUSED_TAB", tabId });
      void guardedInvoke("mux_set_focused_tab", { tabId }).catch(() => undefined);
      return;
    }
    pendingTabSwitchRef.current[workspaceId] = { tabId };
    guardedInvoke("mux_set_active_tab_and_workspace", { workspaceId, tabId }).catch(() => {
      delete pendingTabSwitchRef.current[workspaceId];
      void refreshFromBackend();
    });
    dispatch({ type: "SET_ACTIVE_WORKSPACE", id: workspaceId });
    dispatch({ type: "SET_ACTIVE_TAB", workspaceId, tabId });
  }, [guardedInvoke, refreshFromBackend]);

  // Select + focus a just-spawned tab by resolving its owning tab; a fresh
  // tab may lag local state, so fall back to an authoritative snapshot.
  const selectTabByTab = useCallback(async (workspaceId: string, requestedTabId: string) => {
    const findTab = (s: MultiplexerState) =>
      s.workspaces.find((w) => w.id === workspaceId)?.tabs.find((id) => id === requestedTabId) ?? null;
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
    guardedInvoke("mux_set_focused_tab", { tabId }).catch(() => {});
    dispatch({ type: "SET_FOCUSED_TAB", tabId });
  }, [guardedInvoke, applyBackendState]);

  const setFocusedTab = useCallback((tabId: string) => {
    // Optimistic local update for snappy click feedback.
    dispatch({ type: "SET_FOCUSED_TAB", tabId });
    // Backend re-emits `tab-focused` and keeps `focused_tab_id` authoritative.
    guardedInvoke("mux_set_focused_tab", { tabId }).catch(() => {});
  }, [guardedInvoke]);

  const setTabPty = useCallback((tabId: string, ptyId: string) => {
    dispatch({ type: "SET_TAB_PTY", tabId, ptyId });
  }, []);

  const addNotification = useCallback(
    (notif: Omit<Notification, "id" | "timestamp" | "read">) => {
      dispatch({
        type: "UPSERT_TOAST",
        toast: {
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
    void invoke("mark_notification_read", { id }).catch((err) => {
      console.warn("Failed to mark notification read:", err);
    });
  }, []);

  const markAllRead = useCallback(() => {
    const unreadIds = stateRef.current.notifications
      .filter((notification) => !notification.read)
      .map((notification) => notification.id);
    dispatch({ type: "MARK_ALL_READ" });
    void Promise.all(unreadIds.map((id) => invoke("mark_notification_read", { id }))).catch((err) => {
      console.warn("Failed to mark all notifications read:", err);
    });
  }, []);

  const deleteNotification = useCallback((id: string) => {
    dispatch({ type: "DELETE_NOTIFICATION", id });
    void invoke("clear_notifications", { id }).catch((err) => {
      console.warn("Failed to delete notification:", err);
    });
  }, []);

  // The chosen label is all that travels: the tab it answers to was recorded on
  // the notification when the question was sent. Nothing is rendered optimistically
  // — the answer shown is the one the store recorded, which on a second tap is
  // still the first one.
  const answerNotification = useCallback((id: string, label: string) => {
    void invoke<{ answered?: string; delivered?: boolean }>("answer_notification", { id, label })
      .then((result) => {
        if (result?.answered) {
          dispatch({
            type: "ANSWER_NOTIFICATION",
            id,
            answered: result.answered,
            delivered: result.delivered === true,
          });
        }
      })
      .catch((err) => {
        console.warn("Failed to record notification answer:", err);
      });
  }, []);

  const clearNotifications = useCallback(() => {
    dispatch({ type: "CLEAR_NOTIFICATIONS" });
    void invoke("clear_notifications").catch((err) => {
      console.warn("Failed to clear notifications:", err);
    });
  }, []);

  const dismissToast = useCallback((id: string) => {
    dispatch({ type: "DISMISS_TOAST", id });
  }, []);

  const setTabTitle = useCallback((workspaceId: string, tabId: string, title: string) => {
    const workspace = stateRef.current.workspaces.find((ws) => ws.id === workspaceId);
    if (!workspace?.tabs.includes(tabId)) return;
    dispatch({ type: "SET_TAB_TITLE", tabId, title, level: TitleLevel.User });
  }, []);

  const setAgentType = useCallback((tabId: string, agentType: string | null) => {
    dispatch({ type: "SET_AGENT_TYPE", tabId, agentType });
  }, []);

  const updateTabStatus = useCallback((tabId: string, status: TabStatus) => {
    dispatch({ type: "UPDATE_TAB_STATUS", tabId, status });
  }, []);

  useEffect(() => {
    muxStateStoreRef.current.notify();
  }, [state]);

  useEffect(
    () => mountMuxPerfTestHook(isCodeTermFrontendTestMode(), () => stateRef.current, dispatch),
    [],
  );

  const actions = useMemo<MultiplexerActionsContextType>(
    () => ({
      createWorkspace,
      setActiveWorkspace,
      closeWorkspace,
      renameWorkspace,
      moveTab,
      reorderTabs,
      reorderWorkspaces,
      createTab,
      attachRemoteTab,
      closeTab,
      reopenClosed,
      reopenGroup,
      restoreOrphaned,
      setActiveTab,
      selectTabByTab,
      setFocusedTab,
      setTabPty,
      addNotification,
      upsertToast,
      markRead,
      markAllRead,
      deleteNotification,
      clearNotifications,
      answerNotification,
      dismissToast,
      setTabTitle,
      setAgentType,
      updateTabStatus,
      spawnClaudeTab,
      spawnAgentTab,
      createOrchestrationPair,
    }),
    [
      createWorkspace,
      setActiveWorkspace,
      closeWorkspace,
      renameWorkspace,
      moveTab,
      reorderTabs,
      reorderWorkspaces,
      createTab,
      attachRemoteTab,
      closeTab,
      reopenClosed,
      reopenGroup,
      restoreOrphaned,
      setActiveTab,
      selectTabByTab,
      setFocusedTab,
      setTabPty,
      addNotification,
      upsertToast,
      markRead,
      markAllRead,
      deleteNotification,
      clearNotifications,
      answerNotification,
      dismissToast,
      setTabTitle,
      setAgentType,
      updateTabStatus,
      spawnClaudeTab,
      spawnAgentTab,
      createOrchestrationPair,
    ],
  );

  return (
    <MuxStateStoreContext.Provider value={muxStateStoreRef.current}>
      <MultiplexerActionsContext.Provider value={actions}>
        {children}
      </MultiplexerActionsContext.Provider>
    </MuxStateStoreContext.Provider>
  );
}
