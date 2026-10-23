import { createNoticePublisher } from "../../lib/notices";
import { tabCloseRejectionNotice } from "./tabCloseRejection";
import { sendNotificationAnswer, type AnswerResult } from "../../lib/notificationAnswerSend";
import { isWorkspaceCwdRefusal, spawnErrorMessage } from "../../lib/spawnErrorMessage";
import {
  useCallback,
  useMemo,
  useReducer,
  useEffect,
  useRef,
  type ReactNode,
} from "react";
import { invoke, listen, reopenClosedTab, reopenClosedGroup, restoreOrphanedTab } from "../../lib/bridge";
import { useEntitlementRefresh } from "./useEntitlementRefresh";
import { recordSurfaceActivity, SURFACE_ACTIVITY_EVENT } from "../../lib/surfaceActivity";
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
import { reorderFailureToast } from "./reorderFailure";
import { commitTabRename } from "./tabRename";
import { freeLimitGate, precheckFreeLimit, raiseFreeLimit } from "../../components/billing/freeLimitGate";
import { isFreeLimitRefusal, limitCounts, reusesWorkspace } from "../../components/billing/freeLimitModel";
import { requestOrchestrationPair } from "./orchestrationPair";
import { setPendingCommand, setPendingCwd } from "../../lib/pendingCommands";
import { defaultSpawnProviderId, getAgentDescriptor } from "../../lib/agentDescriptorCache";
import {atomicSpawnPreset} from "../../lib/remoteExecution";
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
  MeshPeersResponse,
} from "../../lib/types";
import { reducer } from "./reducer";
import { acceptsPageFocus, initialPageSelection } from "./pageSelection";
import { loadClientPageSelection } from "../../lib/clientPageSelection";
import { buildInitialState, HYDRATED_GENERATION } from "./initialState";
import { applyPendingTabGuard, type TabGuards } from "./pendingTabGuard";
import { mountMuxPerfTestHook } from "./muxPerfTestHook";
import { isCodeTermFrontendTestMode } from "../../components/Terminal/useChatSession";
import { MultiplexerActionsContext, MuxStateStoreContext, type MultiplexerActionsContextType, type SpawnedTab } from "./context";
import { createMuxStateStore } from "./selectorStore";
import { createRefreshCoalescer } from "./refreshCoalescer";
import { isMuxStatusDelta, statusDeltaAction, type MuxStatusDelta } from "./statusDelta";
import { mountNotificationHistory } from "./notificationHistory";
import { onResync } from "../../lib/resync";
import { EVENT_STREAM_READY } from "../../lib/eventStreamReady";
import { createPeerRebindGate } from "./peerRebindGate";
import type { Tab } from "../../lib/types";
import { remoteControllerSignature, remoteTabsFromOrdinaryProjection, refreshAttachedRemoteAgentStates, shouldRefreshRemoteTabs } from "./remoteTabProjection";
import {
  loadAttachedRemoteTabs,
  hydrateAttachedRemoteTabs,
  projectAttachedRemoteTab,
  reconcileAttachedRemoteTabs,
  saveAttachedRemoteTabs,
  type RemotePeerTabs,
} from "../../lib/remoteTabPortal";
import { onRemoteTransportState } from "../../lib/remoteTabTransportState";
import { resumeRemoteOutputStreams } from "../../lib/outputStream";
import { resumeRemoteTabEvents, subscribeRemoteTabEvent } from "../../lib/remoteTabEvents";
import { spawnFreshGeneralSession, type SessionLaunchIntent } from "../../lib/generalSession";

import { isSnapshotRefusal, isStorageUnavailableError, muxStateEventAction, tabBootStageEventAction } from "./stateEvents";
export { STORAGE_UNAVAILABLE_MESSAGE, isStorageUnavailableError, isSnapshotRefusal, muxStateEventAction, tabBootStageEventAction } from "./stateEvents";

export { useMultiplexer } from "./context";
export { useMuxState, useMuxSelector } from "./selectorStore";
export { highestTitleLevel } from "./tree";

/** True when this runtime already has a cache projection before reconcile. */
export function webBootHasSyncCache(): boolean {
  return IS_WEB && loadCachedStateSync() !== null;
}

export function MultiplexerProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, null, buildInitialState);
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

  const publishNotice = useMemo(() => createNoticePublisher({ upsertToast }), [upsertToast]);
  const stateRef = useRef(state);
  const backendStateRevisionRef = useRef(0);
  stateRef.current = state;
  const muxStateStoreRef = useRef(createMuxStateStore(() => stateRef.current));
  // so one click = exactly one tab (no duplicate on double-fire).
  const spawnInFlight = useRef<Set<string>>(new Set());
  // Web writes wait until async cache discovery or authoritative reconcile.
  const writeGateRef = useRef<WriteGate>(createWriteGate(IS_WEB));
  const reconciledRef = useRef(false);
  // In-flight user tab selections; masks stale snapshots until the backend
  // confirms the switch, rejects it, or removes the target tab.
  const pendingTabSwitchRef = useRef<TabGuards>({});
  const peerSwitchRef = useRef(0);
  const attachedRemoteTabsRef = useRef<Record<string, Tab>>(loadAttachedRemoteTabs());

  const applyRemoteProjection = useCallback((tabs: Record<string, Tab>) => {
    dispatch({ type: "SET_REMOTE_TABS", tabs: reconcileAttachedRemoteTabs(tabs, attachedRemoteTabsRef.current) });
  }, []);

  useEffect(() => {
    if (!IS_WEB) return;
    let active = true;
    void loadCachedStateAsync().then((cached) => {
      if (!active) return;
      if (!cached) {
        if (!reconciledRef.current) writeGateRef.current.unlock();
        return;
      }
      if (reconciledRef.current) return;
      const displayState = { ...cached, generation: HYDRATED_GENERATION };
      dispatch({ type: "SET_STATE", state: displayState, pageOwnedSelection: true,
        initialSelection: initialPageSelection(displayState, loadClientPageSelection()) });
    });
    return () => { active = false; };
  }, []);
  useEffect(() => {
    if (!IS_WEB) return;
    let active = true;
    let contextGeneration = 0;
    const reset = () => {
      contextGeneration++;
      attachedRemoteTabsRef.current = {};
      applyRemoteProjection(remoteTabsFromOrdinaryProjection(Object.values(stateRef.current.tabs)));
    };
    const hydrate = () => {
      const generation = ++contextGeneration;
      void hydrateAttachedRemoteTabs().then((attached) => {
        if (!active || generation !== contextGeneration) return;
        attachedRemoteTabsRef.current = attached;
        applyRemoteProjection(remoteTabsFromOrdinaryProjection(Object.values(stateRef.current.tabs)));
      });
    };
    window.addEventListener("codeterm-peer-switch-reset", reset);
    window.addEventListener("codeterm-peer-switch-rebind", hydrate);
    hydrate();
    return () => { active = false; window.removeEventListener("codeterm-peer-switch-reset", reset); window.removeEventListener("codeterm-peer-switch-rebind", hydrate); };
  }, [applyRemoteProjection]);

  const applyBackendState = useCallback((s: MultiplexerState, force = false) => {
    backendStateRevisionRef.current++;
    // here silently dropped the closed-history on every reload.
    const guarded = applyPendingTabGuard(s, pendingTabSwitchRef.current);
    dispatch({ type: "SET_STATE", state: guarded, force, pageOwnedSelection: IS_WEB,
      initialSelection: IS_WEB && !reconciledRef.current ? initialPageSelection(guarded, loadClientPageSelection()) : undefined });
    const remoteTabs = Object.fromEntries(
      Object.entries(stateRef.current.tabs).filter(([, tab]) => Boolean(tab.remote_peer_id)),
    );
    if (Object.keys(remoteTabs).length > 0) {
      applyRemoteProjection(remoteTabs);
    }
  }, [applyRemoteProjection]);

  const { generation, workspaces, tabs, teams, activeTabIds, active_workspace_id,
    focused_tab_id, recently_closed, closed_structures, tabStatuses } = state;
  // A reload paints the cache at HYDRATED_GENERATION, so a snapshot that changed only the generation is not rewritten.
  const cacheReady = generation > 0;
  useEffect(() => {
    if (!IS_WEB || !cacheReady) return;
    void saveStateSnapshot({ generation: stateRef.current.generation, tabs, teams, active_workspace_id, focused_tab_id,
      recently_closed, closed_structures, workspaces, activeTabIds, tab_statuses: tabStatuses });
  }, [cacheReady, workspaces, tabs, teams, activeTabIds, active_workspace_id,
    focused_tab_id, recently_closed, closed_structures, tabStatuses]);

  const cachedBackendGenerationRef = useRef<number | undefined>(undefined);
  const acceptBackendState = useCallback((s: MultiplexerState, force = false) => {
    if (!force && s.generation !== undefined && cachedBackendGenerationRef.current !== undefined
      && s.generation <= cachedBackendGenerationRef.current) return;
    cachedBackendGenerationRef.current = s.generation;
    const wasReconciled = reconciledRef.current;
    applyBackendState(s, force);
    reconciledRef.current = true;
    writeGateRef.current.unlock();
    peerRebindGateRef.current.acceptedSnapshot(peerSwitchRef.current);
    if (IS_WEB && !wasReconciled) window.dispatchEvent(new Event("codeterm-cache-reconciled"));
  }, [applyBackendState]);

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

  useEffect(() => mountWebLifecycle(), []);

  const remoteProjectionActiveRef = useRef(true);
  const hasPairedPeersRef = useRef<boolean | null>(null);
  const refreshRemoteProjection = useCallback(async () => {
    if (!shouldRefreshRemoteTabs(hasPairedPeersRef.current, stateRef.current.tabs, attachedRemoteTabsRef.current)) return;
    const ordinary = await invoke<Tab[]>("ordinary_tabs_list");
    if (!remoteProjectionActiveRef.current) return;
    const remote = remoteTabsFromOrdinaryProjection(ordinary);
    const projected = await refreshAttachedRemoteAgentStates(
      reconcileAttachedRemoteTabs(remote, attachedRemoteTabsRef.current),
      remote,
      (peerId) => invoke<RemotePeerTabs>("mesh_peer_tabs_list", { peerId }),
    );
    if (!remoteProjectionActiveRef.current) return;
    for (const id of Object.keys(attachedRemoteTabsRef.current)) {
      if (projected[id]) attachedRemoteTabsRef.current[id] = projected[id];
    }
    applyRemoteProjection(projected);
    resumeRemoteOutputStreams();
    resumeRemoteTabEvents();
  }, [applyRemoteProjection]);

  const remoteRefreshRunRef = useRef<() => Promise<boolean>>(() => Promise.resolve(false));
  const remoteRefreshCoalescerRef = useRef(
    createRefreshCoalescer(() => remoteRefreshRunRef.current(), isSnapshotRefusal),
  );
  const requestRemoteRefresh = useCallback(
    (): Promise<boolean> => remoteRefreshCoalescerRef.current.request(),
    [],
  );
  const runRemoteRefresh = useCallback(async (): Promise<boolean> => {
    try {
      await refreshRemoteProjection();
      return remoteProjectionActiveRef.current;
    } catch (error) {
      if (isSnapshotRefusal(error)) throw error;
      console.warn("Failed to refresh ordinary tabs:", error);
      return false;
    }
  }, [refreshRemoteProjection]);
  remoteRefreshRunRef.current = runRemoteRefresh;

  const runRefresh = useCallback(async (force = false): Promise<boolean> => {
    try {
      const startedAt = cachedBackendGenerationRef.current;
      const revision = backendStateRevisionRef.current;
      const s = await invoke<MultiplexerState>("mux_get_state");
      if (s) {
        const unchanged = cachedBackendGenerationRef.current === startedAt && backendStateRevisionRef.current === revision;
        if (force && !unchanged && s.generation !== undefined && s.generation <= (cachedBackendGenerationRef.current ?? -1)) {
          void refreshCoalescerRef.current.request(undefined, true);
        } else acceptBackendState(s, force && unchanged);
      }
      return !!s;
    } catch (e) {
      // Back-pressure, not failure: rethrow so the coalescer records it and
      // the next event's request retries. Never logged as an error.
      if (isSnapshotRefusal(e)) throw e;
      if (isStorageUnavailableError(e)) {
        dispatch({ type: "STORAGE_UNAVAILABLE" });
        return false;
      }
      if (!(e instanceof DOMException && e.name === "AbortError")) {
        console.warn("Failed to refresh state:", e);
      }
      return false;
    }
  }, [acceptBackendState]);
  const runRefreshRef = useRef(runRefresh);
  runRefreshRef.current = runRefresh;
  const refreshCoalescerRef = useRef(
    createRefreshCoalescer((force) => runRefreshRef.current(force), isSnapshotRefusal,
      () => cachedBackendGenerationRef.current),
  );
  const peerRebindGateRef = useRef(
    createPeerRebindGate({
      unlock: () => writeGateRef.current.unlock(),
      announce: (switchId, success) =>
        window.dispatchEvent(new CustomEvent("codeterm-peer-switch-ready", {
          detail: { switchId, success },
        })),
    }),
  );
  const refreshFromBackend = useCallback(
    (generation?: number, force = false): Promise<boolean> => refreshCoalescerRef.current.request(generation, force),
    [],
  );

  const restoreFromBackend = useCallback(() => refreshFromBackend(undefined, true), [refreshFromBackend]);
  useEffect(() => {
    if (!state.muxResyncRequired) return;
    cachedBackendGenerationRef.current = undefined;
    void restoreFromBackend();
  }, [state.muxResyncRequired, restoreFromBackend]);

  useEffect(() => {
    remoteProjectionActiveRef.current = true;
    let active = true;
    let peerReadGeneration = 0;
    let meshUnlisten: (() => void) | undefined;
    let muxUnlisten: (() => void) | undefined;
    const refreshPeers = async () => {
      const generation = ++peerReadGeneration;
      hasPairedPeersRef.current = null;
      try {
        const peers = await invoke<MeshPeersResponse>("mesh_list_peers");
        if (active && generation === peerReadGeneration && Array.isArray(peers?.paired)) {
          hasPairedPeersRef.current = peers.paired.length > 0;
        }
      } catch {}
      if (active && generation === peerReadGeneration) void requestRemoteRefresh();
    };
    const onMeshStateChanged = () => {
      for (const tab of Object.values(stateRef.current.tabs)) {
        if (!tab.remote_peer_id) continue;
        for (const channel of ["control", "output", "chat"] as const) {
          dispatch({ type: "UPDATE_REMOTE_CHANNEL", tabId: tab.id, channel, availability: "reconnecting" });
        }
      }
      void refreshPeers();
    };
    void listen("mesh-state-changed", onMeshStateChanged).then((fn) => {
      if (active) {
        meshUnlisten = fn;
        void refreshPeers();
      } else fn();
    });
    void listen("remote-workers-changed", () => {
      void requestRemoteRefresh();
    }).then((fn) => {
      if (active) muxUnlisten = fn;
      else fn();
    });
    return () => {
      active = false;
      remoteProjectionActiveRef.current = false;
      meshUnlisten?.();
      muxUnlisten?.();
    };
  }, [requestRemoteRefresh]);

  // Live telemetry leaves the controller signature unchanged, so a streaming snapshot never re-reads remote tabs.
  const remoteControllers = useMemo(() => remoteControllerSignature(state.tabs), [state.tabs]);
  const remoteControllersRef = useRef(remoteControllers);
  useEffect(() => {
    if (remoteControllersRef.current === remoteControllers) return;
    remoteControllersRef.current = remoteControllers;
    void requestRemoteRefresh();
  }, [remoteControllers, requestRemoteRefresh]);

  const remotePeerSubscriptions = JSON.stringify(Object.values(state.tabs)
    .filter((tab) => tab.remote_peer_id)
    .sort((a, b) => a.id.localeCompare(b.id))
    .filter((tab, index, tabs) => tabs.findIndex((item) => item.remote_peer_id === tab.remote_peer_id) === index)
    .map((tab) => tab.id));
  useEffect(() => {
    const stops = (JSON.parse(remotePeerSubscriptions) as string[]).map((id) =>
      subscribeRemoteTabEvent(`peer-state-changed:${id}`, () => { void requestRemoteRefresh(); }));
    return () => { for (const stop of stops) stop?.(); };
  }, [remotePeerSubscriptions, requestRemoteRefresh]);

  const attachRemoteTab = useCallback((workspaceId: string, peer: RemotePeerTabs, tab: Tab) => {
    const projected = projectAttachedRemoteTab(tab, peer, workspaceId);
    attachedRemoteTabsRef.current = { ...attachedRemoteTabsRef.current, [projected.id]: projected };
    saveAttachedRemoteTabs(attachedRemoteTabsRef.current);
    applyRemoteProjection(remoteTabsFromOrdinaryProjection(Object.values(stateRef.current.tabs)));
    dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "program", id: workspaceId });
    dispatch({ type: "SET_ACTIVE_TAB", source: "program", workspaceId, tabId: projected.id });
    dispatch({ type: "SET_FOCUSED_TAB", tabId: projected.id });
  }, [applyRemoteProjection]);

  useEffect(() => onRemoteTransportState(({ ordinaryTabId, channel, availability }) => {
    dispatch({ type: "UPDATE_REMOTE_CHANNEL", tabId: ordinaryTabId, channel, availability });
  }), []);

  useEffect(() => {
    const unlisteners: (() => void)[] = [];

    listen<{ paths: string[]; workspace_id: string }>("close-lanes-preserved", ({ payload }) => {
      if (!payload.paths.length) return;
      publishNotice({ kind: "debug", id: uid(),
        tabId: null,
        workspaceId: payload.workspace_id,
        title: "Lanes preserved",
        detail: payload.paths.join("\n"),
        severity: "Info" });
    }).then(unlisten => unlisteners.push(unlisten));

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

    listen<{ id: string; title: string; body: string }>("notification-updated", (event) => {
      dispatch({ type: "UPDATE_NOTIFICATION_SUMMARY", ...event.payload });
    }).then((unlisten) => unlisteners.push(unlisten));

    listen<{ id: string; answered: string; delivered?: boolean; deliveryError?: string; refusal?: { reason: string; detail?: string } }>(
      "notification-answered",
      (event) => {
        if (!event.payload?.id) return;
        dispatch({
          type: "UPDATE_NOTIFICATION_ANSWER",
          id: event.payload.id,
          update: { answered: event.payload.answered, delivered: event.payload.delivered === true,
            error: event.payload.deliveryError ?? event.payload.refusal?.detail ?? event.payload.refusal?.reason },
        });
      },
    ).then((unlisten) => unlisteners.push(unlisten));

    listen<{ id: string }>("notification-resolved", (event) => {
      if (!event.payload?.id) return;
      dispatch({ type: "RESOLVE_NOTIFICATION", id: event.payload.id });
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

    listen<{ tab_id: string; pty_id: string; state: import("../../lib/shellCommandState").ShellCommandState }>(
      "shell-state-changed", (event) => {
        if (!event.payload?.tab_id) return;
        dispatch({ type: "UPDATE_SHELL_STATE", tabId: event.payload.tab_id,
          ptyId: event.payload.pty_id, state: event.payload.state });
        void refreshFromBackend();
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
    const applyMuxEvent = (event: { payload: MultiplexerState | MuxStatusDelta }) => {
      (eventStats.__ctEvents ??= { received: 0, lastAt: 0, reconnects: 0 }).received++;
      eventStats.__ctEvents.lastAt = Date.now();
      const s = event.payload;
      if (isMuxStatusDelta(s)) {
        if (stateRef.current.muxResyncRequired) {
          void refreshFromBackend(s.generation, true);
          return;
        }
        const action = statusDeltaAction(cachedBackendGenerationRef.current, s);
        if (action === "apply") {
          cachedBackendGenerationRef.current = s.generation;
          dispatch({ type: "APPLY_STATUS_DELTA", delta: s });
        } else if (action === "refresh") {
          void refreshFromBackend(s.generation, true);
        }
      } else if (muxStateEventAction(s) === "apply-snapshot") {
        acceptBackendState(s as MultiplexerState);
        // Resize tabs after layout changes so xterms fit new container sizes.
        requestAnimationFrame(() => {
          window.dispatchEvent(new Event("codeterm-tab-resize"));
        });
      } else {
        void refreshFromBackend(s?.generation);
      }
    };
    // The authoritative read follows the bind: a refused read waits for the next
    // state event, and one emitted before the bind would never reach this page.
    const renewMuxListener = () => {
      const generation = ++muxListenGeneration;
      muxUnlisten?.();
      muxUnlisten = null;
      void listen<MultiplexerState | MuxStatusDelta>("mux-state-changed", applyMuxEvent)
        .then((unlisten) => {
          if (generation !== muxListenGeneration) {
            unlisten();
            return;
          }
          muxUnlisten = unlisten;
          (eventStats.__ctEvents ??= { received: 0, lastAt: 0, reconnects: 0 }).reconnects++;
          void refreshFromBackend();
        })
        .catch((error) => {
          console.warn("Failed to renew mux event listener:", error);
          if (generation === muxListenGeneration) void refreshFromBackend();
        });
    };
    renewMuxListener();
    if (isCodeTermFrontendTestMode()) {
      // Test-only seam simulating a stream disconnect by forcing a rebind.
      (eventStats.__ctEvents ??= { received: 0, lastAt: 0, reconnects: 0 }).breakMuxStream = renewMuxListener;
    }
    unlisteners.push(() => {
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
      reconciledRef.current = false;
      cachedBackendGenerationRef.current = undefined;
      dispatch({ type: "RESET_PEER_CONTEXT" });
    };
    const rebindPeerContext = (event: Event) => {
      const switchId = (event as CustomEvent<{ switchId: number }>).detail.switchId;
      peerRebindGateRef.current.open(switchId);
      void refreshFromBackend().then((success) => {
        if (peerSwitchRef.current !== switchId || success) return;
        // A refusal is not-yet: stay open and let a later accepted snapshot
        // settle it. Anything else is a real failure and closes the rebind.
        if (refreshCoalescerRef.current.lastWasRefused()) {
          peerRebindGateRef.current.refused(switchId);
          return;
        }
        peerRebindGateRef.current.failed(switchId);
      });
    };
    window.addEventListener("codeterm-peer-switch-reset", resetPeerContext);
    window.addEventListener("codeterm-peer-switch-rebind", rebindPeerContext);
    unlisteners.push(() => window.removeEventListener("codeterm-peer-switch-reset", resetPeerContext));
    unlisteners.push(() => window.removeEventListener("codeterm-peer-switch-rebind", rebindPeerContext));

    const onBootChanged = () => {
      cachedBackendGenerationRef.current = undefined;
      dispatch({ type: "RESET_GENERATION" });
    };
    window.addEventListener("codeterm-web-boot-changed", onBootChanged);
    unlisteners.push(() => window.removeEventListener("codeterm-web-boot-changed", onBootChanged));

    listen<{ tab_id: string }>("tab-focused", (event) => {
      const tabId = event.payload.tab_id;
      if (!acceptsPageFocus(stateRef.current, tabId, IS_WEB)) return;
      if (stateRef.current.tabs[tabId]) dispatch({ type: "SET_FOCUSED_TAB", tabId });
      else void refreshFromBackend();
    }).then((unlisten) => unlisteners.push(unlisten));

    listen<unknown>(SURFACE_ACTIVITY_EVENT, (event) => recordSurfaceActivity(event.payload))
      .then((unlisten) => unlisteners.push(unlisten));

    const beforeUnload = () => { invoke("save_session").catch(() => {}); };
    window.addEventListener("beforeunload", beforeUnload);

    return () => {
      for (const unlisten of unlisteners) unlisten();
      window.removeEventListener("beforeunload", beforeUnload);
    };
  }, [acceptBackendState, refreshFromBackend, publishNotice]);


  // The mux-state-changed listener above owns the first authoritative read and
  // every read after a refusal, so no timer or caller-side retry is needed.
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

    return mountNotificationHistory(
      () => invoke<Notification[]>("list_notifications", { unreadOnly: false }),
      (notifications) =>
        dispatch({
          type: "HYDRATE_NOTIFICATIONS",
          // Ingress for stored rows: an action written before actions carried
          // daemon provenance loads with its invoke stripped, never accepted.
          notifications: notifications.map((notification) => ({
            ...notification,
            actions: adoptStoredActions(notification.actions),
          })),
        }),
      (refresh) => {
        const unsubscribe = onResync(refresh);
        window.addEventListener(EVENT_STREAM_READY, refresh);
        return () => {
          unsubscribe();
          window.removeEventListener(EVENT_STREAM_READY, refresh);
        };
      },
      (error) => console.warn("Failed to load notification history:", error),
    );
  }, []);

  useEffect(() => freeLimitGate.countWith(() => limitCounts(stateRef.current)), []);
  useEntitlementRefresh(refreshFromBackend);

  const createTab = useCallback(async (workspaceId: string, cwd?: string, shell?: string) => {
    if (precheckFreeLimit(["agent.tabs"])) return undefined;
    try {
      guardedInvoke("mux_set_active_workspace", { workspaceId }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "program", id: workspaceId });
      const ws = stateRef.current.workspaces.find((w) => w.id === workspaceId);
      const effectiveCwd = cwd || ws?.cwd || undefined;
      const tabInfo = await guardedInvoke<{ id: string; title: string }>("mux_create_tab", { workspaceId, activate: true, shell, cwd: effectiveCwd });
      if (effectiveCwd) {
        setPendingCwd(tabInfo.id, effectiveCwd);
        await guardedInvoke("mux_update_tab_cwd", { tabId: tabInfo.id, cwd: effectiveCwd });
      }
      await guardedInvoke("mux_set_active_tab", { workspaceId, tabId: tabInfo.id });
      // Select only once the page holds a snapshot containing the Tab: a page-owned selection drops a Tab the
      // snapshot lacks, and an older in-flight snapshot would otherwise put the previous Tab back.
      const snapshot = await invoke<MultiplexerState>("mux_get_state");
      if (snapshot && snapshot.workspaces.length > 0) applyBackendState(snapshot);
      dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "program", id: workspaceId });
      dispatch({ type: "SET_ACTIVE_TAB", source: "program", workspaceId, tabId: tabInfo.id });
      // Explicit New Tab click focuses the new tab; background spawns don't.
      guardedInvoke("mux_set_focused_tab", { tabId: tabInfo.id }).catch(() => {});
      return { id: tabInfo.id };
    } catch (e) {
      if (!raiseFreeLimit(e, "agent.tabs")) console.error("Failed to create tab:", e);
    }
  }, [guardedInvoke, applyBackendState]);

  const reopenClosed = useCallback(async (workspaceId: string, index = 0) => {
    try {
      const tabInfo = await reopenClosedTab(workspaceId, index);
      dispatch({ type: "REMOVE_PENDING_CLOSE", tabIds: [tabInfo.tab_id] });
      guardedInvoke("mux_set_active_tab_and_workspace", { workspaceId, tabId: tabInfo.tab_id }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "program", id: workspaceId });
      dispatch({ type: "SET_ACTIVE_TAB", source: "program", workspaceId, tabId: tabInfo.tab_id });
      guardedInvoke("mux_set_focused_tab", { tabId: tabInfo.tab_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to reopen closed tab:", e);
    }
  }, [refreshFromBackend, guardedInvoke]);

  const reopenGroup = useCallback(async (groupId: string) => {
    try {
      const result = await reopenClosedGroup(groupId);
      const restored = await guardedInvoke<MultiplexerState>("mux_get_state");
      const tabIds = Object.values(restored.tabs).filter((tab) =>
        tab.close_restore?.record_id === groupId || tab.id === result.tab_id,
      ).map((tab) => tab.id);
      dispatch({ type: "REMOVE_PENDING_CLOSE", tabIds });
      guardedInvoke("mux_set_active_workspace", { workspaceId: result.workspace_id }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "program", id: result.workspace_id });
      dispatch({ type: "SET_ACTIVE_TAB", source: "program", workspaceId: result.workspace_id, tabId: result.tab_id });
      guardedInvoke("mux_set_focused_tab", { tabId: result.tab_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to reopen closed group:", e);
      throw e;
    }
  }, [refreshFromBackend, guardedInvoke]);

  const restoreOrphaned = useCallback(async (tabId: string) => {
    try {
      const result = await restoreOrphanedTab(tabId);
      dispatch({ type: "REMOVE_PENDING_CLOSE", tabIds: [tabId, result.tab_id] });
      guardedInvoke("mux_set_active_workspace", { workspaceId: result.workspace_id }).catch(() => {});
      dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "program", id: result.workspace_id });
      dispatch({ type: "SET_ACTIVE_TAB", source: "program", workspaceId: result.workspace_id, tabId: result.tab_id });
      guardedInvoke("mux_set_focused_tab", { tabId: result.tab_id }).catch(() => {});
      refreshFromBackend();
    } catch (e) {
      console.error("Failed to restore orphaned tab:", e);
    }
  }, [refreshFromBackend, guardedInvoke]);

  const createWorkspace = useCallback(async (name: string, cwd?: string, opts?: { activate?: boolean }) => {
    if (!reusesWorkspace(stateRef.current.workspaces, cwd) && precheckFreeLimit(["workspaces"])) return undefined;
    try {
      // Reuse existing workspace for the same cwd; avoid duplicate tabs.
      const activate = opts?.activate ?? true;
      const info = await guardedInvoke<{ id: string; name: string }>("mux_create_workspace", { name, cwd: cwd ?? null, activate });
      const s = await invoke<MultiplexerState>("mux_get_state");
      if (s && s.workspaces.length > 0) applyBackendState(s);
      // A page-owned selection ignores the daemon's active workspace, so the page selects what it created.
      if (activate) dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "program", id: info.id });
      return info;
    } catch (e) {
      if (!raiseFreeLimit(e, "workspaces")) console.error("Failed to create workspace:", e);
      return undefined;
    }
  }, [guardedInvoke, applyBackendState]);

  const setActiveWorkspace = useCallback((id: string) => {
    guardedInvoke("mux_set_active_workspace", { workspaceId: id }).catch(() => {});
    dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "user", id });
  }, [guardedInvoke]);

  const closeWorkspace = useCallback(
    function closeWorkspace(id: string, force = false) {
      const ws = stateRef.current.workspaces.find((w) => w.id === id);
      const tabIds = ws ? [...ws.tabs] : [];
      dispatch({ type: "CLOSE_WORKSPACE", id });
      guardedInvoke("mux_close_workspace", { workspaceId: id, force })
        .then(() => refreshFromBackend())
        .catch(async () => {
          const revision = backendStateRevisionRef.current;
          const current = await invoke<MultiplexerState>("mux_get_state").catch(() => null);
          if (!current) return;
          const remaining = current.workspaces.find(workspace => workspace.id === id);
          dispatch({ type: "REMOVE_PENDING_CLOSE", tabIds });
          acceptBackendState(current, current.generation === cachedBackendGenerationRef.current && revision === backendStateRevisionRef.current);
          if (!remaining) return;
          const survivors = remaining.tabs.map(tabId => current.tabs[tabId]?.title || tabId);
          publishNotice({ kind: "toast", id: uid(),
            tabId: null,
            workspaceId: id,
            title: "Workspace close failed",
            detail: survivors.length ? survivors.join("\n") : remaining.name,
            severity: "Error",
            actions: [{ label: "Retry", run: () => closeWorkspace(id, true) }] });
          void restoreFromBackend();
        });
    },
    [guardedInvoke, refreshFromBackend, restoreFromBackend, acceptBackendState, publishNotice],
  );

  const renameWorkspace = useCallback((id: string, name: string) => {
    guardedInvoke("mux_rename_workspace", { workspaceId: id, name }).catch(() => {});
    dispatch({ type: "RENAME_WORKSPACE", id, name });
  }, [guardedInvoke]);

  const moveTab = useCallback((tabId: string, fromWorkspaceId: string, toWorkspaceId: string) => {
    guardedInvoke("mux_move_tab", { tabId, fromWorkspaceId, toWorkspaceId }).catch(() => { void restoreFromBackend(); });
    dispatch({ type: "MOVE_TAB", tabId, fromWorkspaceId, toWorkspaceId });
  }, [guardedInvoke, restoreFromBackend]);

  // The optimistic order stays until the daemon's state arrives; a refused save snaps back now and says why.
  const commitReorder = useCallback((command: string, args: Record<string, unknown>, workspaceId: string | null) => {
    guardedInvoke(command, args).catch((error: unknown) => {
      void restoreFromBackend();
      publishNotice(reorderFailureToast(workspaceId, error));
    });
  }, [guardedInvoke, restoreFromBackend, publishNotice]);

  const reorderTabs = useCallback(
    (args: { workspaceId?: string; parentTabId?: string; orderedTabIds: string[] }) => {
      dispatch({ type: "REORDER_TABS", orderedTabIds: args.orderedTabIds });
      if (args.parentTabId) {
        commitReorder("mux_reorder_child", { parentTabId: args.parentTabId, orderedTabIds: args.orderedTabIds }, args.workspaceId ?? null);
      } else if (args.workspaceId) {
        commitReorder("mux_reorder_tab", { workspaceId: args.workspaceId, orderedTabIds: args.orderedTabIds }, args.workspaceId);
      }
    },
    [commitReorder],
  );

  const reorderDisplay = useCallback((args: { workspaceId: string; orderedTabIds: string[] }) => {
    dispatch({ type: "REORDER_DISPLAY", orderedTabIds: args.orderedTabIds });
    commitReorder("mux_reorder_display", args, args.workspaceId);
  }, [commitReorder]);

  const reorderWorkspaces = useCallback((orderedWorkspaceIds: string[]) => {
    dispatch({ type: "REORDER_WORKSPACES", orderedWorkspaceIds });
    commitReorder("mux_reorder_workspace", { orderedWorkspaceIds }, null);
  }, [commitReorder]);

  const setWorkspaceCwd = useCallback(async (workspaceId: string, cwd: string) => {
    const result = await guardedInvoke<{ warnings: string[] }>("mux_set_workspace_cwd", { workspaceId, cwd });
    const snapshot = await guardedInvoke<MultiplexerState>("mux_get_state");
    applyBackendState(snapshot);
    if (result.warnings.length > 0) publishNotice({ kind: "toast", id: uid(),
      tabId: null,
      workspaceId: workspaceId,
      title: "Workspace folder updated",
      detail: result.warnings.join("\n"),
      severity: "Warning" });
    return result;
  }, [guardedInvoke, applyBackendState, publishNotice]);

  const spawnAgentTab = useCallback(async (
    workspaceId: string,
    agentId: string,
    cwd?: string,
    model?: string,
    reasoningEffort?: string,
    preset?: string,
    intent?: SessionLaunchIntent,
    executionMode?: string,
  ): Promise<SpawnedTab | null> => {
    if (precheckFreeLimit(["agent.tabs"])) return null;
    if (intent === "fresh_general") {
      const guardKey = `${workspaceId}:fresh_general`;
      if (spawnInFlight.current.has(guardKey)) return null;
      spawnInFlight.current.add(guardKey);
      try {
        const result = await spawnFreshGeneralSession(workspaceId, agentId, cwd, guardedInvoke, { model, reasoningEffort, preset });
        applyBackendState(result.snapshot);
        return { workspaceId: result.workspace_id, tabId: result.tab_id };
      } catch (error) {
        raiseFreeLimit(error, "agent.tabs");
        refreshFromBackend();
        throw error;
      } finally {
        spawnInFlight.current.delete(guardKey);
      }
    }
    const guardKey = `${workspaceId}:${agentId}`;
    if (spawnInFlight.current.has(guardKey)) return null;
    spawnInFlight.current.add(guardKey);
    try {
      const res = await guardedInvoke<AtomicSpawn>("mux_spawn_tab", {
        params: { agentId, workspaceId, cwd, model, reasoningEffort, ...atomicSpawnPreset(getAgentDescriptor(agentId), preset, executionMode) },
      });
      applyBackendState(res.snapshot);
      if (res.launch_command) {
        if (cwd) setPendingCwd(res.tab_id, cwd);
        setPendingCommand(res.tab_id, res.launch_command);
      }
      pendingTabSwitchRef.current[res.workspace_id] = { tabId: res.tab_id };
      dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "program", id: res.workspace_id });
      dispatch({ type: "SET_ACTIVE_TAB", source: "program", workspaceId: res.workspace_id, tabId: res.tab_id });
      // Snappy local focus; backend also emits the authoritative tab-focused.
      dispatch({ type: "SET_FOCUSED_TAB", tabId: res.tab_id });
      return { workspaceId: res.workspace_id, tabId: res.tab_id };
    } catch (e) {
      raiseFreeLimit(e, "agent.tabs");
      refreshFromBackend();
      throw e;
    } finally {
      spawnInFlight.current.delete(guardKey);
    }
  }, [refreshFromBackend, guardedInvoke, guardedWebJson, applyBackendState]);

  // from the descriptor registry rather than hardcoding a vendor id.
  const spawnClaudeTab = useCallback(async (workspaceId: string, cwd?: string) => {
    void spawnAgentTab(workspaceId, defaultSpawnProviderId(), cwd).catch((error) => {
      if (isWorkspaceCwdRefusal(error) || isFreeLimitRefusal(error)) return;
      publishNotice({ kind: "toast", id: uid(),
        tabId: null,
        workspaceId: workspaceId,
        title: "Session start failed",
        detail: spawnErrorMessage(error),
        severity: "Error" });
    });
  }, [spawnAgentTab, publishNotice]);

  const createOrchestrationPair = useCallback(
    async (input: OrchestrationPairInput): Promise<OrchestrationPairResult> => {
      const result = await requestOrchestrationPair(guardedInvoke, input);
      refreshFromBackend();
      return result;
    },
    [guardedInvoke, refreshFromBackend],
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
            void requestRemoteRefresh();
          }
          else refreshFromBackend();
        })
        .catch((error: unknown) => {
          dispatch({ type: "REMOVE_PENDING_CLOSE", tabIds: [tabId] });
          publishNotice(tabCloseRejectionNotice(error, workspaceId, tabId));
          if (isRemote) void requestRemoteRefresh();
          else void restoreFromBackend();
        });
    },
    [refreshFromBackend, restoreFromBackend, requestRemoteRefresh, guardedInvoke, publishNotice],
  );

  const setActiveTab = useCallback((workspaceId: string, tabId: string) => {
    const remote = stateRef.current.tabs[tabId];
    if (remote?.remote_peer_id) {
      dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "user", id: workspaceId });
      dispatch({ type: "SET_ACTIVE_TAB", source: "user", workspaceId, tabId });
      dispatch({ type: "SET_FOCUSED_TAB", tabId });
      void guardedInvoke("mux_set_focused_tab", { tabId }).catch(() => undefined);
      return;
    }
    pendingTabSwitchRef.current[workspaceId] = { tabId };
    guardedInvoke("mux_set_active_tab_and_workspace", { workspaceId, tabId }).catch(() => {
      delete pendingTabSwitchRef.current[workspaceId];
      void restoreFromBackend();
    });
    dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "user", id: workspaceId });
    dispatch({ type: "SET_ACTIVE_TAB", source: "user", workspaceId, tabId });
    if (IS_WEB) dispatch({ type: "SET_FOCUSED_TAB", tabId });
  }, [guardedInvoke, restoreFromBackend]);

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
    dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "program", id: workspaceId });
    dispatch({ type: "SET_ACTIVE_TAB", source: "program", workspaceId, tabId });
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

  const pendingAnswers = useRef(new Set<string>());
  const answerNotification = useCallback((id: string, label: string) => {
    void sendNotificationAnswer(
      id, label, pendingAnswers.current,
      stateRef.current.notifications.find((entry) => entry.id === id),
      (answerId, answerLabel) => invoke<AnswerResult>("answer_notification", { id: answerId, label: answerLabel }),
      (answerId, update) => dispatch({ type: "UPDATE_NOTIFICATION_ANSWER", id: answerId, update }),
    );
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

  const renameTab = useCallback((workspaceId: string, tabId: string, title: string) => {
    void commitTabRename(stateRef.current, {
      dispatch,
      persist: (args) => guardedInvoke("mux_rename_tab", args),
      notify: publishNotice,
    }, workspaceId, tabId, title);
  }, [guardedInvoke, publishNotice]);

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
      reorderDisplay,
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
      upsertToast,
      markRead,
      markAllRead,
      deleteNotification,
      clearNotifications,
      answerNotification,
      dismissToast,
      renameTab,
      setAgentType,
      updateTabStatus,
      spawnClaudeTab,
      spawnAgentTab,
      setWorkspaceCwd,
      createOrchestrationPair,
    }),
    [
      createWorkspace,
      setActiveWorkspace,
      closeWorkspace,
      renameWorkspace,
      moveTab,
      reorderTabs,
      reorderDisplay,
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
      upsertToast,
      markRead,
      markAllRead,
      deleteNotification,
      clearNotifications,
      answerNotification,
      dismissToast,
      renameTab,
      setAgentType,
      updateTabStatus,
      spawnClaudeTab,
      spawnAgentTab,
      setWorkspaceCwd,
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
