import { frontendBootDiagnostic } from "./lib/frontendBootDiagnostic";
import { useFrontendApp } from "./lib/frontendUpdate/useFrontendApp";
import { memo, useState, useEffect, useCallback, useMemo, useRef } from "react";
import { invoke, IS_WEB } from "./lib/bridge";
import { buildTeamActions } from "./lib/teamActions";
import { selectAiHelperTab } from "./lib/aiHelperFocus";
import { getCurrentTabUiMode, resolveInitialTabUiMode } from "./lib/clientTabUiMode";
import { tabChatCapable } from "./lib/chatUiMode";
import { pruneOrphanComposerDrafts } from "./lib/composerDraftGc";
import { Titlebar } from "./components/Titlebar/Titlebar";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { SidebarDrawer } from "./components/Sidebar/SidebarDrawer";
import { TabContent } from "./components/Terminal/TabContent";
import { LocalRecordingCaptureStatus } from "./components/Terminal/voice/LocalRecordingCaptureStatus";
import { LocalRecordingSetup } from "./components/Terminal/voice/LocalRecordingSetup";
import { useLocalRecordingCapture } from "./components/Terminal/voice/useLocalRecordingCapture";
import { SettingsPanel } from "./components/Settings/SettingsPanel";
import { isSettingsLinkId, type SettingsLinkId } from "./components/Settings/sections";
import { ChatBlockHostProvider, GuideNavigationProvider } from "@codeterm/chat-ui";
import { appChatBlockHost } from "./lib/chatBlocks/appChatBlockHost";
import { AppChatBlockServices } from "./lib/chatBlocks/AppChatBlockServices";
import { GUIDE_SECTIONS, guideRequest, type GuideRequest } from "./help/guideNavigation";
import type { AgentLevelChoice } from "./lib/types";
import { MemoryPanel, type MemoryInitialState } from "./components/MemoryPanel/MemoryPanel";
import { MeshPanel } from "./components/MeshPanel/MeshPanel";
import { ExtensionsView } from "./components/Extensions/ExtensionsView";
import { AnalyticsSupporterSection } from "./components/Supporter/AnalyticsSupporterSection";
import { PluginView } from "./components/Extensions/PluginView";
import { ANALYTICS_PLUGIN_ID, isAnalyticsAvailable } from "./components/Titlebar/analyticsTrayLogic";
import { usePluginSnapshotStore } from "./lib/pluginSnapshotStore";
import { pluginViewBelongsToActiveTab, pluginViewScope, type PluginViewScope } from "./components/Extensions/pluginViewScope";
import { NavigationBackBar } from "./components/Navigation/NavigationBackBar";
import { navigationStore } from "./components/Navigation/navigationStore";
import { workerFocusStore } from "./components/MemoryPanel/workerFocusStore";
import { CommandPalette, buildDefaultCommands } from "./components/CommandPalette/CommandPalette";
import { RecoveryStatusBanner } from "./components/CrashReportToast";
import { FrontendUpdateNotice } from "./components/FrontendUpdateNotice";
import { StaleBuildBanner } from "./components/StaleBuildBanner";
import { UpdateProgressLayer } from "./components/UpdateProgressLayer";
import { CanvasView } from "./components/Canvas/CanvasView";
import { EssenceMapCanvas } from "./components/Canvas/EssenceMapCanvas";
import { PlansProvider } from "./components/Canvas/PlansProvider";
import { usePlanPeerContext } from "./components/Canvas/usePlanPeerContext";
import { PlanSidePanelHost } from "./components/Canvas/PlanSidePanelHost";
import { canvasFocusStore, useCanvasFocusOpen, useCanvasFocusTarget, useCanvasWorkspaceDismissed, useCanvasWorkspaceOpen } from "./components/Canvas/canvasFocusStore";
import { useBlueprintRunPopups } from "./components/Automation/useBlueprintRunPopups";
import { JobsView, type JobsTarget } from "./components/Jobs/JobsView";
import { useReturnToChat } from "./hooks/useReturnToChat";
import type { ReturnTo } from "./lib/returnTo";
import { OPEN_JOB_EVENT, type OpenJobDetail } from "./components/Jobs/openJobEvent";
import { jobsEntryFilter, jobsOpenEntry, type JobsOpenScope } from "./components/Jobs/jobsLayout";
import { CronWorkspaceView } from "./components/Cron/CronWorkspaceView";
import { cronSelectionStore, useCronSelection } from "./components/Cron/cronSelectionStore";
import { useConfig } from "./hooks/useConfig";
import { useHelperMomentsHost } from "./lib/helperMoments/helperMomentsWiring";
import { useNotificationSound } from "./hooks/useNotificationSound";
import { useKeyboardInset } from "./hooks/useKeyboardInset";
import { useSidebarViewport } from "./hooks/useSidebarViewport";
import { useTabPrewarm } from "./hooks/terminal/useTabPrewarm";
import { usageTrackingOverrides } from "./components/Terminal/usageTracking";
import { useAgentProviderCachePublisher } from "./hooks/useAgentProviders";
import { usePluginProviderMetaSync } from "./hooks/usePluginProviderMeta";
import { UpdateProvider } from "./hooks/useUpdate";
import { usePluginActivity } from "./lib/usePluginActivity";
import {
  MultiplexerProvider,
  useMultiplexer,
  useMuxState,
} from "./store/multiplexer";
import { defaultSpawnProviderId } from "./lib/agentDescriptorCache";
import { openNewSessionPicker } from "./lib/newSessionChoice";
import { matchesBinding } from "./lib/keybindings";
import { forwardTerminalFallback } from "./lib/terminalFallbackInput";
import { writeTerminalInput } from "./hooks/terminal/terminalInputWrite";
import { cacheThemeSnapshot, isLightColor, themeToCSS } from "./lib/themeCss";
import { nativeFolderDialog as openDialog } from "./lib/nativeBridge";
import { fontSizeToUiFontScale } from "./lib/uiFontScale";
import { DEV_BUILD } from "./lib/devBuild";
// TODO: Agent Swarm Visualization — future feature import { SwarmOverlay } from "./components/Swarm/SwarmOverlay";

/** One stable empty list, so a config still loading does not hand the chime a
 *  fresh array on every render. */
const EMPTY_LEVELS: AgentLevelChoice[] = [];

const MemoizedTabContent = memo(TabContent);

function jsonEquals<T>(a: T, b: T): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function useStableJsonValue<T>(value: T): T {
  const ref = useRef(value);
  if (!jsonEquals(ref.current, value)) {
    ref.current = value;
  }
  return ref.current;
}

function AppInner() {
  useAgentProviderCachePublisher();
  usePluginProviderMetaSync();
  const { activities: pluginActivities, displayMap: pluginActivityDisplayMap } = usePluginActivity();
  // Phone width owns the sidebar: it becomes an overlay drawer and yields the viewport to the content until the hamburger reopens it.
  const {
    open: sidebarOpen,
    mounted: sidebarMounted,
    shown: sidebarShown,
    toggle: toggleSidebar,
    close: closeSidebar,
    closeOnPhone: closeSidebarOnMobile,
    phone: phoneViewport,
  } = useSidebarViewport();
  const [sidebarRailCollapsed, setSidebarRailCollapsed] = useState(false);
  const [sidebarExpandRequest, setSidebarExpandRequest] = useState(0);

  const viewLayerStyle = (active: boolean): React.CSSProperties => ({
    zIndex: active ? 2 : 0,
    opacity: active ? 1 : 0,
    visibility: active ? "visible" : "hidden",
    pointerEvents: active ? "auto" : "none",
    transition: active
      ? "opacity 200ms ease, visibility 0ms linear 0ms"
      : "opacity 200ms ease, visibility 0ms linear 200ms",
  });
  const { inset: keyboardInset, keyboardOpen } = useKeyboardInset();
  const [mainView, setMainView] = useState<"workspace" | "settings" | "memory" | "mesh" | "extensions" | "analytics">("workspace");
  const automationOpener = useRef<HTMLElement | null>(null);
  const [automationWorkspaceId, setAutomationWorkspaceId] = useState<string | null>(null);
  const [jobsTarget, setJobsTarget] = useState<(JobsTarget & { request: number; returnTo?: ReturnTo }) | null>(null);
  const [jobsScope, setJobsScope] = useState<JobsOpenScope>("workspace");
  const [workspaceMapWorkspaceId, setWorkspaceMapWorkspaceId] = useState<string | null>(null);
  const [pluginViewId, setPluginViewId] = useState<string | null>(null);
  const [pluginViewCwd, setPluginViewCwd] = useState<string | null>(null);
  const [pluginViewOwner, setPluginViewOwner] = useState<PluginViewScope | null>(null);
  const [settingsSection, setSettingsSection] = useState<SettingsLinkId | undefined>(undefined);
  const [requestedGuide, setRequestedGuide] = useState<GuideRequest | undefined>(undefined);
  const [memoryInitial, setMemoryInitial] = useState<MemoryInitialState | undefined>(undefined);
  // Lazy-mount panels: mount once on first open, stay mounted for instant switching.
  const [mountedSettings, setMountedSettings] = useState(false);
  const [mountedMemory, setMountedMemory] = useState(false);
  const [mountedMesh, setMountedMesh] = useState(false);
  const [mountedExtensions, setMountedExtensions] = useState(false);
  const [mountedAnalytics, setMountedAnalytics] = useState(false);
  useEffect(() => {
    if (mainView === "settings") setMountedSettings(true);
    if (mainView === "memory") setMountedMemory(true);
    if (mainView === "mesh") setMountedMesh(true);
    if (mainView === "extensions") setMountedExtensions(true);
    if (mainView === "analytics") setMountedAnalytics(true);
  }, [mainView]);
  const analyticsButtonRef = useRef<HTMLButtonElement>(null);
  const { snapshot: pluginSnapshot, hasLoaded: pluginSnapshotLoaded } = usePluginSnapshotStore();
  const analyticsAvailable = isAnalyticsAvailable(pluginSnapshot.plugins);
  useEffect(() => {
    if (pluginSnapshotLoaded && !analyticsAvailable) {
      setMainView((cur) => (cur === "analytics" ? "workspace" : cur));
    }
  }, [analyticsAvailable, pluginSnapshotLoaded]);
  const closeAnalytics = useCallback(() => {
    setMainView((cur) => (cur === "analytics" ? "workspace" : cur));
    analyticsButtonRef.current?.focus();
  }, []);
  const setSettingsOpen = useCallback((next: boolean | ((v: boolean) => boolean)) => {
    setMainView((cur) => {
      const wasOpen = cur === "settings";
      const open = typeof next === "function" ? next(wasOpen) : next;
      return open ? "settings" : "workspace";
    });
  }, []);
  const openAgentsSettings = useCallback(() => {
    setSettingsSection("agents");
    setSettingsOpen(true);
  }, [setSettingsOpen]);
  const openGuide = (sectionId: string) => {
    setRequestedGuide(guideRequest(sectionId));
    setSettingsSection("guide");
    setSettingsOpen(true);
  };
  const setMemoryOpen = useCallback((next: boolean | ((v: boolean) => boolean)) => {
    setMainView((cur) => {
      const wasOpen = cur === "memory";
      const open = typeof next === "function" ? next(wasOpen) : next;
      return open ? "memory" : "workspace";
    });
  }, []);
  const setMeshOpen = useCallback((next: boolean | ((v: boolean) => boolean)) => {
    setMainView((cur) => {
      const wasOpen = cur === "mesh";
      const open = typeof next === "function" ? next(wasOpen) : next;
      return open ? "mesh" : "workspace";
    });
  }, []);
  const setExtensionsOpen = useCallback((next: boolean | ((v: boolean) => boolean)) => {
    setMainView((cur) => {
      const wasOpen = cur === "extensions";
      const open = typeof next === "function" ? next(wasOpen) : next;
      return open ? "extensions" : "workspace";
    });
  }, []);
  const { config, updateConfig, theme, loading: configLoading } = useConfig();
  const stableTheme = useStableJsonValue(theme);
  const stableFontConfig = useStableJsonValue(config.font);
  const stableTerminalConfig = useStableJsonValue(config.terminal);
  const stableApiConfig = useStableJsonValue(config.api);
  const uiFontScale = fontSizeToUiFontScale(stableFontConfig.size);
  useNotificationSound(
    Boolean(config.notifications?.sound),
    config.notifications?.agent_levels ?? EMPTY_LEVELS,
  );

  // events on its own (the per-bubble `useConfig` was multiplying invokes by tab count and made the webview unresponsive after many resumes).
  const computedUsageEnabledByProvider = useMemo(() => {
    return usageTrackingOverrides(config.agents ?? {});
  }, [config.agents]);
  const usageEnabledByProvider = useStableJsonValue(computedUsageEnabledByProvider);

  const {
    createWorkspace,
    createTab,
    closeTab,
    reopenClosed,
    setFocusedTab,
    setActiveTab,
    selectTabByTab,
    setTabPty,
    setActiveWorkspace,
    markRead,
    markAllRead,
    deleteNotification,
    clearNotifications,
    answerNotification,
    dismissToast,
    upsertToast,
    spawnClaudeTab,
  } = useMultiplexer();
  const state = useMuxState();
  const larkCapture = useLocalRecordingCapture({ tabs: state.tabs });
  // The capture listener's readiness promise lives for the hook's lifetime; readiness is reported once.
  const larkReadyRef = useRef(larkCapture.ready);
  larkReadyRef.current = larkCapture.ready;
  useEffect(() => {
    if (!IS_WEB) {
      void (async () => {
        frontendBootDiagnostic("ready-request");
        await larkReadyRef.current;
        const generation = await invoke<number>("frontend_readiness_generation");
        const accepted = await invoke<boolean>("frontend_ready", { generation });
        frontendBootDiagnostic(accepted ? "ready-accepted" : "ready-rejected");
        if (!accepted) throw new Error("frontend readiness generation was replaced");
      })().catch((error) => {
        frontendBootDiagnostic("ready-failed");
        console.warn("Unable to report frontend readiness", error);
      });
    }
  }, []);
  useFrontendApp({
    generation: state.generation,
    workspaceId: state.active_workspace_id,
    tabId: state.active_workspace_id ? state.activeTabIds[state.active_workspace_id] ?? null : null,
    hasWorkspace: id => state.workspaces.some(ws => ws.id === id),
    hasTab: id => !!state.tabs[id],
    selectWorkspace: setActiveWorkspace,
    selectTab: setActiveTab,
  });

  useHelperMomentsHost(state.tabs, configLoading ? null : config.integrations?.telegram_enabled ?? false);
  const peerContext = usePlanPeerContext();
  useBlueprintRunPopups(state.workspaces, peerContext.generation, state.toasts, upsertToast);
  const navigateToNotificationTab = useCallback(
    (tabId: string, workspaceId: string | null) => {
      if (workspaceId) {
        void selectTabByTab(workspaceId, tabId);
        return;
      }
      setFocusedTab(tabId);
    },
    [selectTabByTab, setFocusedTab],
  );

  const lastFocusedRef = useRef(state.focused_tab_id);
  useEffect(() => {
    if (state.focused_tab_id !== lastFocusedRef.current) {
      lastFocusedRef.current = state.focused_tab_id;
      if (state.focused_tab_id != null && mainView !== "workspace") {
        setMainView("workspace");
      }
    }
  }, [state.focused_tab_id, mainView]);

  const stateRef = useRef(state);
  stateRef.current = state;
  const teamActions = useMemo(
    () => buildTeamActions({
      invoke,
      callerNonce: (tabId) => stateRef.current.tabs[tabId]?.session_binding_nonce ?? undefined,
      selectedTab: () => stateRef.current.focused_tab_id,
    }),
    [],
  );

  // Reclaim composer drafts left behind by closed tabs so a reused/colliding
  // tab id can never inherit a dead tab's unsent text.
  useEffect(() => {
    if (typeof window === "undefined") return;
    try {
      pruneOrphanComposerDrafts(window.localStorage, new Set(Object.keys(state.tabs)));
    } catch {
      // Best-effort: draft GC must never break the app.
    }
  }, [state.tabs]);
  const configRef = useRef(config);
  configRef.current = config;

  // (e.g. the Git plugin's "Open Git" button, or a glance's "Open settings").
  useEffect(() => {
    const onOpenPanel = (e: Event) => {
      const detail = (e as CustomEvent<{
        panel?: string;
        cwd?: string | null;
        memoryState?: MemoryInitialState;
      }>).detail;
      const panel = detail?.panel;
      if (panel?.startsWith("view:")) {
        const fp = stateRef.current.focused_tab_id
          ? stateRef.current.tabs[stateRef.current.focused_tab_id]
          : null;
        setPluginViewCwd(detail?.cwd || fp?.cwd || null);
        setPluginViewOwner(pluginViewScope(stateRef.current.active_workspace_id, stateRef.current.activeTabIds));
        setPluginViewId(panel.slice("view:".length));
      } else if (panel === "extensions") {
        setExtensionsOpen(true);
      } else if (panel?.startsWith("settings:") && isSettingsLinkId(panel.slice("settings:".length))) {
        setSettingsSection(panel.slice("settings:".length) as SettingsLinkId);
        setSettingsOpen(true);
      } else if (panel === "memory") {
        if (DEV_BUILD && import.meta.env.VITE_CODETERM_TEST === "1") setMemoryInitial(detail?.memoryState);
        setMemoryOpen(true);
      }
    };
    window.addEventListener("codeterm:open-panel", onOpenPanel);
    return () => window.removeEventListener("codeterm:open-panel", onOpenPanel);
  }, [setExtensionsOpen]);

  // dismiss any covering Extensions/glance overlay and focus the new tab.
  useEffect(() => {
    const onFocusTab = (e: Event) => {
      const detail = (e as CustomEvent<{ tabId?: string; workspaceId?: string }>).detail;
      const tabId = detail?.tabId;
      if (!tabId) return;
      setExtensionsOpen(false);
      if (detail.workspaceId) selectAiHelperTab(detail.workspaceId, tabId, setActiveTab, setFocusedTab);
      else setFocusedTab(tabId);
    };
    window.addEventListener("codeterm:focus-tab", onFocusTab);
    return () => window.removeEventListener("codeterm:focus-tab", onFocusTab);
  }, [setActiveTab, setExtensionsOpen, setFocusedTab]);

  const handleBackToMemory = useCallback(() => {
    const ret = navigationStore.get();
    const focused = stateRef.current.focused_tab_id;
    if (focused) workerFocusStore.unfocus(focused);
    if (ret) {
      setMemoryInitial({
        selectedId: ret.memoryState.selectedNodeId,
        feedVisible: ret.memoryState.feedVisible,
        workerPanelVisible: ret.memoryState.workerPanelVisible,
      });
    }
    setMemoryOpen(true);
    navigationStore.clear();
  }, []);

  const handleMemoryFocusTab = useCallback((tabId: string) => {
    setFocusedTab(tabId);
  }, []);

  const activeWs = useMemo(
    () => state.workspaces.find((ws) => ws.id === state.active_workspace_id),
    [state.workspaces, state.active_workspace_id],
  );

  const activeTab = useMemo(
    () => {
      const activeTabId = activeWs ? state.activeTabIds[activeWs.id] : null;
      return activeTabId ? state.tabs[activeTabId] : undefined;
    },
    [activeWs, state.activeTabIds, state.tabs],
  );
  const activePluginViewScope = useMemo(
    () => pluginViewScope(state.active_workspace_id, state.activeTabIds),
    [state.active_workspace_id, state.activeTabIds],
  );
  const closePluginView = useCallback(() => {
    setPluginViewId(null);
    setPluginViewCwd(null);
    setPluginViewOwner(null);
  }, []);
  useEffect(() => {
    if (pluginViewId && !pluginViewBelongsToActiveTab(pluginViewOwner, activePluginViewScope)) {
      closePluginView();
    }
  }, [activePluginViewScope, closePluginView, pluginViewId, pluginViewOwner]);
  const toggleAnalytics = useCallback(() => {
    closePluginView();
    setMainView((cur) => (cur === "analytics" ? "workspace" : "analytics"));
  }, [closePluginView]);
  const prewarmBackendTab = useCallback(async (tabId: string) => {
    const current = stateRef.current;
    const tab = current.tabs[tabId];
    if (!tab || tab.backing?.kind === "chat_backend" || !tab.agent_type) return;
    await invoke("spawn_pty", {
      tabId: tab.id,
      cwd: tab.cwd ?? undefined,
      cols: 120,
      rows: 30,
    }).catch((error) => {
      console.warn(`[prewarm] failed to activate backend for tab ${tabId}:`, error);
    });
  }, []);
  const prewarmTabIds = useTabPrewarm({
    workspaceId: activeWs?.id ?? null,
    activeTabId: activeTab?.id ?? null,
    tabIds:
      activeWs?.tabs
        .filter((tabId) => {
          const tab = state.tabs[tabId];
          return tab && !(activeWs.protected === true && tab.protected === true);
        })
        ?? [],
    liveTabIds: state.workspaces.flatMap((ws) => ws.tabs),
    prewarmTab: prewarmBackendTab,
  });
  const showWorkspaceCanvas =
    mainView === "workspace" && !pluginViewId && activeWs != null && !activeTab;
  const workspaceCanvasOpen = useCanvasWorkspaceOpen();
  const workspaceCanvasDismissed = useCanvasWorkspaceDismissed();
  const showInlineWorkspaceCanvas = showWorkspaceCanvas && !workspaceCanvasDismissed;
  const showOverlayWorkspaceCanvas =
    workspaceCanvasOpen &&
    !!activeTab &&
    mainView === "workspace" &&
    !pluginViewId &&
    activeWs != null;
  const workspaceCanvasVisible = showInlineWorkspaceCanvas || showOverlayWorkspaceCanvas;
  const cronSelection = useCronSelection();
  const closeWorkspaceMap = useCallback(() => {
    setWorkspaceMapWorkspaceId(null);
  }, []);
  const activateReturnTo = useReturnToChat();
  const closeWorkspaceAutomation = useCallback(() => {
    setAutomationWorkspaceId(null);
    if (automationOpener.current?.isConnected) automationOpener.current.focus();
  }, []);
  const openWorkspaceAutomation = useCallback((workspaceId: string, target?: JobsTarget, scope: JobsOpenScope = "workspace", returnTo?: ReturnTo) => {
    if (document.activeElement instanceof HTMLElement) automationOpener.current = document.activeElement;
    closePluginView();
    setActiveWorkspace(workspaceId);
    canvasFocusStore.closeWorkspace();
    canvasFocusStore.close();
    cronSelectionStore.close();
    setWorkspaceMapWorkspaceId(null);
    setMainView("workspace");
    setAutomationWorkspaceId(workspaceId);
    setJobsScope(scope);
    setJobsTarget((current) => target ? { ...target, returnTo, request: (current?.request ?? 0) + 1 } : null);
    closeSidebarOnMobile();
  }, [closePluginView, closeSidebarOnMobile, setActiveWorkspace]);
  useEffect(() => {
    const onOpenJob = (event: Event) => {
      const detail = (event as CustomEvent<OpenJobDetail>).detail;
      if (detail?.workspaceId) openWorkspaceAutomation(detail.workspaceId, detail.target, "workspace", detail.returnTo);
    };
    window.addEventListener(OPEN_JOB_EVENT, onOpenJob);
    return () => window.removeEventListener(OPEN_JOB_EVENT, onOpenJob);
  }, [openWorkspaceAutomation]);
  const openWorkspaceMap = useCallback((workspaceId: string) => {
    setAutomationWorkspaceId(null);
    closePluginView();
    setActiveWorkspace(workspaceId);
    canvasFocusStore.closeWorkspace();
    cronSelectionStore.close();
    setMainView("workspace");
    setWorkspaceMapWorkspaceId(workspaceId);
    closeSidebarOnMobile();
  }, [closePluginView, closeSidebarOnMobile, setActiveWorkspace]);
  const handleNavigateToTab = useCallback(() => {
    setAutomationWorkspaceId(null);
    closeWorkspaceMap();
    canvasFocusStore.closeWorkspace();
    cronSelectionStore.close();
    setMainView("workspace");
    closeSidebarOnMobile();
  }, [closeSidebarOnMobile, closeWorkspaceMap]);
  useEffect(() => {
    if (mainView !== "workspace") setWorkspaceMapWorkspaceId(null);
  }, [mainView]);
  useEffect(() => {
    if (mainView !== "workspace" || cronSelection || (automationWorkspaceId && !state.workspaces.some((workspace) => workspace.id === automationWorkspaceId))) setAutomationWorkspaceId(null);
  }, [mainView, cronSelection, automationWorkspaceId, state.workspaces]);
  useEffect(() => {
    if (!activeTab) canvasFocusStore.resetWorkspaceDismissed();
  }, [activeTab]);
  const canvasFocusOpen = useCanvasFocusOpen();
  const canvasFocusTarget = useCanvasFocusTarget();
  const handleNewTab = useCallback(() => {
    if (!activeWs) return;
    const focusedTab = state.focused_tab_id ? state.tabs[state.focused_tab_id] : null;
    const cwd = focusedTab?.cwd || undefined;
    createTab(activeWs.id, cwd);
  }, [activeWs, state.focused_tab_id, state.tabs, createTab]);

  const handleCloseCurrentTab = useCallback(() => {
    if (activeWs && activeTab) {
      closeTab(activeWs.id, activeTab.id);
    }
  }, [activeWs, activeTab, closeTab]);

  const handlePtySpawned = useCallback(
    (tabId: string, ptyId: string) => {
      invoke("mux_link_pty", { tabId, ptyId }).catch(() => {});
      setTabPty(tabId, ptyId);
    },
    [setTabPty],
  );

  const paletteCommands = useMemo(
    () =>
      buildDefaultCommands({
        newTab: handleNewTab,
        closeTab: handleCloseCurrentTab,
        newWorkspace: () => {
          openDialog({ directory: true, multiple: false, title: "Choose workspace folder" })
            .then((folder) => {
              if (!folder) return;
              const basename = folder.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "Workspace";
              createWorkspace(basename, folder);
            })
            .catch(() => {});
        },
        openSettings: () => { setSettingsSection(undefined); setSettingsOpen(true); },
        toggleSidebar,
        spawnClaude: async () => {
          const wsId = stateRef.current.active_workspace_id ?? stateRef.current.workspaces[0]?.id;
          if (!wsId) return;
          if (config.agent.full_access === undefined) {
            const workspace = stateRef.current.workspaces.find((candidate) => candidate.id === wsId);
            openNewSessionPicker({
              workspaceId: wsId,
              workspaceCwd: workspace?.cwd ?? null,
              initialProviderId: defaultSpawnProviderId(),
            });
            return;
          }
          let folder: string | null;
          try {
            folder = await openDialog({
              directory: true,
              multiple: false,
              title: "Choose project folder for Claude",
            });
          } catch {
            return;
          }
          if (!folder) return;
          spawnClaudeTab(wsId, folder);
        },
      }),
    [
      handleNewTab,
      handleCloseCurrentTab,
      createWorkspace,
      config.agent.full_access,
      spawnClaudeTab,
      toggleSidebar,
    ],
  );

  const kb = config.keybindings;
  const kbKey = JSON.stringify(kb);
  const activeWorkspaceId = activeWs?.id ?? null;
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Config-driven bindings (language-independent via e.code)
      // Browser-standard reopen-closed-tab. Most recent = ring index 0.
      if (matchesBinding(e, kb.reopen_tab ?? "Ctrl+Shift+T")) {
        e.preventDefault();
        if (activeWorkspaceId) reopenClosed(activeWorkspaceId, 0);
        return;
      }

      if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;

      // FALLBACK: If xterm.js textarea doesn't have focus, send keys directly to PTY.
      // Skip when the focused tab is in Chat UI mode — keys shouldn't leak into the PTY.
      const active = document.activeElement;
      if (stateRef.current.focused_tab_id) {
        const focusedTabId = stateRef.current.focused_tab_id;
        const tab = stateRef.current.tabs[focusedTabId];
        if (tab?.pty_id) {
          const tabUiMode = getCurrentTabUiMode(focusedTabId)
            ?? resolveInitialTabUiMode(focusedTabId, { chatCapable: tabChatCapable(tab) });
          forwardTerminalFallback(e, active, (data) => writeTerminalInput(tab.id, tab.pty_id!, data), tabUiMode);
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    kbKey,
    activeWorkspaceId,
    reopenClosed,
  ]);

  const themeStyles = useMemo(() => themeToCSS(stableTheme), [stableTheme]);
  const isCRT = stableTheme.name.toLowerCase().replace(/[\s-]/g, "") === "pipboy";

  // Set color-scheme + theme vars on documentElement so portals (modals, popups rendered into document.body via createPortal) inherit them.
  useEffect(() => {
    const isLight = isLightColor(stableTheme.background);
    document.documentElement.style.colorScheme = isLight ? "light" : "dark";
    document.documentElement.dataset.ctTheme = isLight ? "light" : "dark";
    for (const [k, v] of Object.entries(themeStyles)) {
      document.documentElement.style.setProperty(k, v);
    }
    document.querySelector('meta[name="theme-color"]')?.setAttribute("content", stableTheme.background);
    cacheThemeSnapshot(stableTheme);
    document.documentElement.style.fontSize = `${stableFontConfig.size + 2}px`;
  }, [stableTheme.background, themeStyles, stableFontConfig.size]);

  return (
    <GuideNavigationProvider sections={GUIDE_SECTIONS} onOpen={openGuide}>
    <ChatBlockHostProvider value={appChatBlockHost}>
    <div
      className={`flex flex-col w-screen overflow-hidden ${isCRT ? "crt-mode" : ""}`}
      style={{
        ...themeStyles,
        backgroundColor: stableTheme.background,
        color: stableTheme.foreground,
        // covers iOS Safari, where the keyboard shrinks only the VISUAL viewport and dvh stays put (see useKeyboardInset).
        height: keyboardInset ? `calc(100% - ${keyboardInset}px)` : "100%",
        // keyboardOpen (not keyboardInset) drives this: in standalone the inset reads 0 even with the keyboard up, which would leave a dead safe-area stripe under the composer.
        "--ct-composer-bottom-padding": keyboardOpen
          ? "0px"
          : "calc(0.5rem + var(--ct-safe-bottom, 0px))",
      } as React.CSSProperties}
    >
      {/* Native + web mirror share one titlebar strip (parity decision A) — its
          own --ct-safe-top padding covers the web notch/status-bar inset. */}
      <Titlebar
        api={stableApiConfig}
        onToggleSidebar={sidebarRailCollapsed && sidebarOpen ? () => setSidebarExpandRequest((count) => count + 1) : toggleSidebar}
        showSidebarToggle={!sidebarOpen || sidebarRailCollapsed}
        onNewTab={handleNewTab}
        theme={stableTheme}
        onOpenSettings={(section) => { setSettingsSection(section); setSettingsOpen(true); }}
        onOpenExtensions={() => { setExtensionsOpen(true); }}
        analyticsOpen={mainView === "analytics"}
        onToggleAnalytics={toggleAnalytics}
        analyticsButtonRef={analyticsButtonRef}
        notifications={state.notifications}
        showBell={config.notifications?.show_bell !== false}
        toasts={state.toasts}
        onMarkNotificationRead={markRead}
        onMarkAllNotificationsRead={markAllRead}
        onDeleteNotification={deleteNotification}
        onClearNotifications={clearNotifications}
        onAnswerNotification={answerNotification}
        onDismissToast={dismissToast}
        onNavigateToast={navigateToNotificationTab}
      />
      <PlansProvider api={stableApiConfig} enabled={!configLoading}>
      <AppChatBlockServices api={stableApiConfig}>
      <div
        data-testid="app-shell-body"
        data-shell-viewport={phoneViewport ? "phone" : "desktop"}
        className="flex flex-1 min-h-0 relative"
      >
        <LocalRecordingCaptureStatus state={larkCapture.state} />
        {larkCapture.setupPluginId && <LocalRecordingSetup pluginId={larkCapture.setupPluginId} onClose={larkCapture.closeSetup} />}
        {sidebarMounted && (
          <>
            {/* Narrow shell: the sidebar overlays the content with a tap-to-close
                backdrop; wide shell it participates in the flex row as before. */}
            <div
              className={`shell-wide:hidden fixed inset-0 z-30 bg-black/40 ct-sidebar-backdrop ${sidebarShown ? "is-open" : ""}`}
              onClick={closeSidebar}
              aria-hidden={!sidebarShown}
              style={{ pointerEvents: sidebarOpen ? "auto" : "none" }}
            />
            <SidebarDrawer phone={phoneViewport} shown={sidebarShown}>
              <Sidebar
                onCollapse={closeSidebar}
                onCollapsedChange={setSidebarRailCollapsed}
                expandRequest={sidebarExpandRequest}
                onOpenMemory={() => { setMemoryOpen(true); closeSidebarOnMobile(); }}
                onOpenMesh={() => { setMeshOpen(true); closeSidebarOnMobile(); }}
                onOpenExtensions={() => { setExtensionsOpen(true); closeSidebarOnMobile(); }}
                onNavigateToTab={handleNavigateToTab}
                mainView={mainView}
                config={config}
                uiFontScale={uiFontScale}
                workspaceCanvasActive={workspaceCanvasVisible || !!cronSelection || workspaceMapWorkspaceId !== null || automationWorkspaceId !== null}
                onOpenWorkspaceMap={openWorkspaceMap}
                onOpenWorkspaceAutomation={openWorkspaceAutomation}
                teamActions={teamActions}
              />
            </SidebarDrawer>
          </>
        )}

        <div className="flex-1 min-w-0 min-h-0 flex flex-col">
        <main className="flex-1 min-w-0 min-h-0 relative overflow-hidden">
          {/* All panels stay mounted — switching mainView only changes visibility.
              Unmounting the workspace would tear down xterm + re-run get_tab_text
              on return, blocking the main thread with MB of escape codes. */}
          {mountedSettings && (
            <div
              className="absolute inset-0"
              style={viewLayerStyle(mainView === "settings")}
              aria-hidden={mainView !== "settings"}
              inert={mainView !== "settings" || undefined}
            >
              <SettingsPanel
                config={config}
                onUpdateConfig={updateConfig}
                onClose={() => { setSettingsSection(undefined); setSettingsOpen(false); }}
                initialSection={settingsSection}
                guideRequest={requestedGuide}
                tabs={state.tabs}
                workspaces={state.workspaces}
                teams={state.teams}
              />
            </div>
          )}
          {mountedMemory && (
            <div
              className="absolute inset-0"
              style={viewLayerStyle(mainView === "memory")}
              aria-hidden={mainView !== "memory"}
              inert={mainView !== "memory" || undefined}
            >
              <MemoryPanel
                api={stableApiConfig}
                onClose={() => {
                  setMemoryOpen(false);
                  setMemoryInitial(undefined);
                }}
                initialState={memoryInitial}
                onFocusTab={handleMemoryFocusTab}
                config={config}
                onConfigUpdate={updateConfig}
                onOpenAgentsSettings={openAgentsSettings}
              />
            </div>
          )}
          {mountedMesh && (
            <div
              className="absolute inset-0"
              style={viewLayerStyle(mainView === "mesh")}
              aria-hidden={mainView !== "mesh"}
              inert={mainView !== "mesh" || undefined}
            >
              <MeshPanel
                api={stableApiConfig}
                config={config}
                onConfigUpdate={updateConfig}
                onClose={() => setMeshOpen(false)}
              />
            </div>
          )}
          {mountedExtensions && (
            <div
              className="absolute inset-0"
              style={viewLayerStyle(mainView === "extensions")}
              aria-hidden={mainView !== "extensions"}
              inert={mainView !== "extensions" || undefined}
            >
              <ExtensionsView
                visible={mainView === "extensions"}
                pluginActivities={pluginActivities}
                pluginActivityDisplayMap={pluginActivityDisplayMap}
              />
            </div>
          )}
          {mountedAnalytics && analyticsAvailable && (
            <div
              data-testid="analytics-view"
              className="absolute inset-0 flex flex-col"
              style={viewLayerStyle(mainView === "analytics")}
              aria-hidden={mainView !== "analytics"}
              inert={mainView !== "analytics" || undefined}
            >
              <div className="relative min-h-0 flex-1">
                <PluginView
                  pluginId={ANALYTICS_PLUGIN_ID}
                  api={stableApiConfig}
                  active={mainView === "analytics"}
                  focusOnActive
                  kpiNotices
                  onClose={closeAnalytics}
                />
              </div>
              <AnalyticsSupporterSection api={stableApiConfig} />
            </div>
          )}
          {pluginViewId && (
            <div className="absolute inset-0 z-30">
              <PluginView pluginId={pluginViewId} api={config.api} cwd={pluginViewCwd} onClose={closePluginView} />
            </div>
          )}
          {automationWorkspaceId && mainView === "workspace" && (
            <div className="absolute inset-0" style={{ zIndex: 26 }}>
              <JobsView key={`${automationWorkspaceId}:${jobsScope}:${jobsTarget?.request ?? 0}`} workspaces={state.workspaces} teams={state.teams} tabs={state.tabs} initialFilter={jobsEntryFilter(jobsOpenEntry(automationWorkspaceId, jobsScope, jobsTarget?.teamId))} initialScenarioId={jobsTarget?.scenarioId} initialRunId={jobsTarget?.runId} initialCronId={jobsTarget?.cronId} initialBlueprintId={jobsTarget?.blueprintId} returnTo={jobsTarget?.returnTo} onReturnTo={activateReturnTo} onClose={closeWorkspaceAutomation} />
            </div>
          )}
          {workspaceMapWorkspaceId && mainView === "workspace" && (
            <div className="absolute inset-0" style={{ zIndex: 26 }}>
              <EssenceMapCanvas workspaceId={workspaceMapWorkspaceId} onClose={closeWorkspaceMap} />
            </div>
          )}
          {showInlineWorkspaceCanvas && (
            <div className="absolute inset-0" style={{ zIndex: 1 }}>
              <CanvasView
                mode="workspace"
                workspaceId={activeWs?.id}
                api={stableApiConfig}
              />
            </div>
          )}
          {showOverlayWorkspaceCanvas && (
            <div className="absolute inset-0" style={{ zIndex: 25 }}>
              <CanvasView
                mode="workspace"
                workspaceId={activeWs?.id}
                api={stableApiConfig}
              />
            </div>
          )}
          {canvasFocusOpen && canvasFocusTarget && (
            <div className="absolute inset-0" style={{ zIndex: 25 }}>
              <CanvasView
                mode="focus"
                planId={canvasFocusTarget.planId}
                orchestratorTabId={canvasFocusTarget.orchestratorTabId}
                focusTabId={canvasFocusTarget.focusTabId}
                api={stableApiConfig}
                onClose={() => canvasFocusStore.close()}
              />
            </div>
          )}
          {cronSelection && mainView === "workspace" && (
            <div className="absolute inset-0" style={{ zIndex: 24 }}>
              <CronWorkspaceView cronId={cronSelection.cronId} />
            </div>
          )}
          {state.storageUnavailable ? (
            <div
              className="absolute inset-0 flex items-center justify-center px-6 text-center text-sm"
              data-testid="storage-unavailable-empty"
              role="alert"
              style={{ color: "var(--ct-fg)" }}
            >
              Storage unavailable, your data is preserved, see the incident.
            </div>
          ) : state.workspaces.length > 0 ? (
            state.workspaces.flatMap((ws) =>
              ws.tabs.map((tabId) => {
                const tab = state.tabs[tabId];
                if (!tab) return null;
                const isActiveTab = ws.id === state.active_workspace_id && tabId === state.activeTabIds[ws.id];
                // lazy — mount it on boot so its chat/SSE state is live.
                const isMainTab =
                  ws.protected === true && tab.protected === true;
                const shouldMountTab = isActiveTab || isMainTab || prewarmTabIds.has(tabId);
                if (!shouldMountTab) return null;
                const isVisible = mainView === "workspace" && !pluginViewId && isActiveTab && !workspaceCanvasVisible;
                return (
              <div
                key={tabId}
                className="absolute inset-0"
                inert={!isVisible || !!automationWorkspaceId || undefined}
                style={{
                  zIndex: isVisible ? 1 : 0,
                  opacity: isVisible ? 1 : 0,
                  pointerEvents: isVisible ? "auto" : "none",
                  transition: "opacity 200ms ease",
                }}
              >
                <MemoizedTabContent
                  tab={tab}
                  isFocused={state.focused_tab_id === tabId}
                  onFocusTab={setFocusedTab}
                  onPtySpawned={handlePtySpawned}
                  theme={stableTheme}
                  fontConfig={stableFontConfig}
                  terminalConfig={stableTerminalConfig}
                  usageEnabledByProvider={usageEnabledByProvider}
                  api={stableApiConfig}
                  workspaceCwd={ws.cwd ?? null}
                  workspaceId={ws.id}
                  uiFontScale={uiFontScale}
                  isVisible={isVisible}
                />
              </div>
                );
              })
            )
          ) : mainView === "workspace" ? (
            <CanvasView mode="workspace" api={stableApiConfig} />
          ) : null}
          <PlanSidePanelHost api={stableApiConfig} />
        </main>
        </div>
      </div>
      </AppChatBlockServices>
      </PlansProvider>

      <CommandPalette commands={paletteCommands} />

      <RecoveryStatusBanner />

      <FrontendUpdateNotice />

      <StaleBuildBanner />

      <NavigationBackBar onBack={handleBackToMemory} />

      <UpdateProgressLayer />
    </div>
    </ChatBlockHostProvider>
    </GuideNavigationProvider>
  );
}

export function App() {
  return (
    <MultiplexerProvider>
      <UpdateProvider>
        <AppInner />
      </UpdateProvider>
    </MultiplexerProvider>
  );
}
