import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { open } from "@tauri-apps/plugin-dialog";
import { IS_WEB } from "../../lib/webApi";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useMultiplexer, useMuxState } from "../../store/multiplexer";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { getAgentProvider } from "../../lib/agentProviders";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import type { AppConfig } from "../../lib/types";
import { anchorsTeam } from "../../lib/roleKind";
import { TerminalIcon, FolderIcon, FolderPlusIcon, ExtensionsIcon } from "../Icons";
import { AgentIcon } from "../AgentIcon";
import { OrchestrationCreate, type OrchestrationCreateResult } from "./OrchestrationCreate";
import { WorkspaceItem } from "./WorkspaceItem";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { RecentlyClosed } from "./RecentlyClosed";
import {
  SidebarDnd,
  sortByOrder,
  WS_PREFIX,
} from "./sortable";
import { RemoteFolderPicker } from "../shared/RemoteFolderPicker";
import { LiquidFlip } from "../shared/LiquidToggle";
import type { MeshPresence } from "./meshPresenceHeader";
import { flattenSidebarTabs, isSidebarTabVisible, tabsInWorkspace } from "./workspaceTabs";
import { aggregateTabStatuses } from "./tabStatuses";
import { usePlanIndexOptional, useSidebarPlansOptional } from "../Canvas/PlansProvider";
import { selectActiveWork } from "../Canvas/planView/activeWork";
import { isWorkspaceTabActive } from "./sidebarSelection";
import { buildSidebarCycleItems, cycleStep } from "./sidebarCycle";
import { matchesBinding } from "../../lib/keybindings";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";
import { WorkspaceCardGrid } from "./WorkspaceCardGrid";
import { loadSelectedWorkspaceId, persistSelectedWorkspaceId } from "./workspaceCardMetadata";
import type { SidebarTeamActions } from "./TeamUnit";
import { GroupCloseConfirmDialog } from "./GroupCloseConfirmDialog";
import { WorkspaceCloseConfirmDialog } from "./WorkspaceCloseConfirmDialog";
import type { OrchestrationGroupMember } from "./orchestrationGroupClose";
import { hydrateProtectedExpanded, initialSidebarExpanded, partitionSidebarWorkspaces } from "./sidebarSections";
import { NewSessionPicker } from "../Terminal/NewSessionPicker";
import { OPEN_NEW_SESSION_EVENT, type OpenNewSessionDetail } from "../../lib/newSessionChoice";
import { WebPeerPicker } from "../WebPeerPicker/WebPeerPicker";
import { isFirstRunGeneralTab } from "../AgentPicker";
import { RemoteTabPicker } from "./RemoteTabPicker";
import { declaredAttentionByTab } from "./declaredAttention";

export { buildHierarchyTree, buildHierarchySubtree, HierarchyTree } from "./HierarchyTree";

export const SIDEBAR_WORKSPACES_SECTION_EXPANDED_KEY = "codeterm.sidebar.workspaces-section-expanded";

export function loadWorkspacesSectionExpanded(): boolean {
  try {
    return window.localStorage.getItem(SIDEBAR_WORKSPACES_SECTION_EXPANDED_KEY) !== "0";
  } catch {
    return true;
  }
}

export function persistWorkspacesSectionExpanded(expanded: boolean) {
  try {
    window.localStorage.setItem(SIDEBAR_WORKSPACES_SECTION_EXPANDED_KEY, expanded ? "1" : "0");
  } catch {}
}

export function workspaceSectionBodyVisible(isCompact: boolean, expanded: boolean): boolean {
  return isCompact || expanded;
}

export function toggleWorkspacesSectionExpanded(expanded: boolean): boolean {
  return !expanded;
}

/** Stable target for the collapse handle's aria-controls — the sidebar is a singleton region. */
export const SIDEBAR_REGION_ID = "codeterm-sidebar";

interface SidebarProps {
  onCollapse?: () => void;
  onOpenMemory?: () => void;
  onOpenMesh?: () => void;
  onOpenExtensions?: () => void;
  onNavigateToTab?: () => void;
  mainView?: "workspace" | "settings" | "memory" | "mesh" | "git" | "extensions";
  config?: AppConfig;
  uiFontScale?: number;
  workspaceCanvasActive?: boolean;
  teamActions?: Partial<SidebarTeamActions>;
}

export function Sidebar({
  onCollapse,
  onOpenMemory,
  onOpenMesh,
  onOpenExtensions,
  onNavigateToTab,
  mainView = "workspace",
  config,
  uiFontScale,
  workspaceCanvasActive = false,
  teamActions,
}: SidebarProps) {
  const {
    createWorkspace,
    setActiveWorkspace,
    closeWorkspace,
    renameWorkspace,
    reorderTabs,
    reorderWorkspaces,
    createTab,
    attachRemoteTab,
    closeTab,
    reopenClosed,
    reopenGroup,
    restoreOrphaned,
    setActiveTab,
    setTabTitle,
    spawnAgentTab,
    createOrchestrationPair,
    selectTabByTab,
    addNotification,
  } = useMultiplexer();
  const state = useMuxState();
  const declaredAttention = useMemo(
    () => declaredAttentionByTab(state.notifications),
    [state.notifications],
  );
  const isWorkspaceMode = mainView === "workspace";
  const [folderPicker, setFolderPicker] = useState<{
    title: string;
    initialPath?: string | null;
    onSelect: (path: string) => void;
  } | null>(null);
  const [orchestrationModal, setOrchestrationModal] = useState<{ workspaceId: string } | null>(null);
  const [pendingWorkspaceClose, setPendingWorkspaceClose] = useState<{
    id: string;
    name: string;
    agentTitles: string[];
  } | null>(null);
  const [sessionPicker, setSessionPicker] = useState<{
    workspaceId: string;
    workspaceCwd: string | null;
    initialProviderId?: string;
  } | null>(null);
  const [remotePickerWorkspaceId, setRemotePickerWorkspaceId] = useState<string | null>(null);
  const { providers, loading: providersLoading } = useAgentProviders();
  const { index: planIndex } = usePlanIndexOptional();
  const fullPlans = useSidebarPlansOptional();
  const activeWork = useMemo(
    () => selectActiveWork(fullPlans.plans, state.tabs),
    [fullPlans.plans, state.tabs],
  );
  const agentsConfig = config?.agents;
  const visibleProviders = useMemo(
    () => providers.filter((provider) => agentsConfig?.[provider.id]?.visible_in_menu !== false),
    [providers, agentsConfig],
  );

  const handleAgentSpawn = useCallback(async (
    agentId: string,
    workspaceId: string,
    workspaceCwd: string | null,
  ) => {
    if (config?.agent.full_access === undefined) {
      setSessionPicker({ workspaceId, workspaceCwd, initialProviderId: agentId });
      return;
    }
    let folder: string | null = workspaceCwd;
    if (!folder) {
      const provider = getAgentProvider(agentId);
      if (IS_WEB) {
        setFolderPicker({
          title: `Project folder for ${provider?.displayName ?? agentId}`,
          initialPath: workspaceCwd,
          onSelect: (path) => {
            spawnAgentTab(workspaceId, agentId, path);
          },
        });
        return;
      } else {
        try {
          folder = await open({
            directory: true,
            multiple: false,
            title: `Choose project folder for ${provider?.displayName ?? agentId}`,
          });
        } catch (err) {
          console.error(`[agent-spawn] folder picker failed:`, err);
          return;
        }
      }
      if (!folder) return;
    }
    spawnAgentTab(workspaceId, agentId, folder);
  }, [config?.agent.full_access, spawnAgentTab]);

  const handleOrchestrationConfirm = useCallback((result: OrchestrationCreateResult) => {
    if (!orchestrationModal) return;
    const workspaceId = orchestrationModal.workspaceId;
    setOrchestrationModal(null);
    createOrchestrationPair({ ...result, workspaceId }).then((created) => {
      void selectTabByTab(workspaceId, created.orchestrator_tab_id);
    }).catch((e) => {
      addNotification({
        tab_id: null,
        workspace_id: workspaceId,
        title: "Orchestration pair failed",
        body: e instanceof Error ? e.message : String(e),
        severity: "Error",
        level: "failure",
        sticky: true,
      });
    });
  }, [addNotification, createOrchestrationPair, orchestrationModal, selectTabByTab]);

  const firstRunActive = useMemo(
    () => Object.values(state.tabs).some(isFirstRunGeneralTab),
    [state.tabs],
  );

  const handleNewSession = useCallback((workspaceId: string, workspaceCwd: string | null) => {
    if (firstRunActive) return;
    setSessionPicker({ workspaceId, workspaceCwd });
  }, [firstRunActive]);

  const requestWorkspaceClose = useCallback((workspaceId: string) => {
    const workspace = state.workspaces.find((workspace) => workspace.id === workspaceId);
    if (!workspace) return;
    const agentTitles = workspace.tabs.reduce<string[]>((titles, tabId) => {
      const tab = state.tabs[tabId];
      if (tab?.agent_type) titles.push(tab.title);
      return titles;
    }, []);
    if (agentTitles.length === 0) {
      closeWorkspace(workspaceId);
      return;
    }
    setPendingWorkspaceClose({ id: workspaceId, name: workspace.name, agentTitles });
  }, [closeWorkspace, state.tabs, state.workspaces]);

  useEffect(() => {
    const openRequestedSession = (event: Event) => {
      if (firstRunActive) return;
      const detail = (event as CustomEvent<OpenNewSessionDetail>).detail;
      if (!detail?.workspaceId) return;
      setSessionPicker(detail);
    };
    window.addEventListener(OPEN_NEW_SESSION_EVENT, openRequestedSession);
    return () => window.removeEventListener(OPEN_NEW_SESSION_EVENT, openRequestedSession);
  }, [firstRunActive]);

  useEffect(() => {
    if (firstRunActive) setSessionPicker(null);
  }, [firstRunActive]);

  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Set<string>>(
    () => initialSidebarExpanded(state.workspaces, state.active_workspace_id),
  );
  const [workspacesSectionExpanded, setWorkspacesSectionExpanded] = useState(loadWorkspacesSectionExpanded);
  const hydratedProtectedWorkspaceIdsRef = useRef<Set<string>>(new Set());
  const [meshEnabledOverride, setMeshEnabledOverride] = useState<boolean | null>(null);
  useEffect(() => {
    const hydrated = hydrateProtectedExpanded(
      expandedWorkspaces,
      hydratedProtectedWorkspaceIdsRef.current,
      state.workspaces,
    );
    if (hydrated.hydratedProtectedIds.size !== hydratedProtectedWorkspaceIdsRef.current.size) {
      hydratedProtectedWorkspaceIdsRef.current = hydrated.hydratedProtectedIds;
      setExpandedWorkspaces(hydrated.expanded);
    }
  }, [expandedWorkspaces, state.workspaces]);

  const handleNewWorkspace = useCallback(async () => {
    let folder: string | null;
    if (IS_WEB) {
      setFolderPicker({
        title: "Workspace folder",
        onSelect: async (path) => {
          const basename = path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "Workspace";
          await createWorkspace(basename, path);
        },
      });
      return;
    } else {
      try {
        folder = await open({ directory: true, multiple: false, title: "Choose workspace folder" });
      } catch {
        return;
      }
    }
    if (!folder) return;
    const basename = folder.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "Workspace";
    await createWorkspace(basename, folder);
  }, [createWorkspace]);

  useEffect(() => {
    setMeshEnabledOverride(null);
  }, [config?.mesh.enabled]);

  const meshPresence: MeshPresence = (meshEnabledOverride ?? config?.mesh.enabled ?? false) ? "online" : "offline";
  const setMeshEnabled = useCallback(async (nextEnabled: boolean) => {
    if ((meshPresence === "online") === nextEnabled) return;
    setMeshEnabledOverride(nextEnabled);
    try {
      await invoke("mesh_set_enabled", { enabled: nextEnabled });
    } catch {
      setMeshEnabledOverride(null);
    }
  }, [meshPresence]);

  const toggleExpanded = useCallback((wsId: string) => {
    setExpandedWorkspaces((prev) => {
      const next = new Set(prev);
      if (next.has(wsId)) {
        next.delete(wsId);
      } else {
        next.add(wsId);
      }
      return next;
    });
  }, []);

  const toggleWorkspacesSection = useCallback(() => {
    setWorkspacesSectionExpanded((previous) => {
      const next = toggleWorkspacesSectionExpanded(previous);
      persistWorkspacesSectionExpanded(next);
      return next;
    });
  }, []);

  const [sidebarWidth, setSidebarWidth] = useState(220);
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false);
  const lastExpandedWidth = useRef(220);
  const sidebarDragging = useRef(false);
  const sidebarMoved = useRef(false);
  const collapseOnRelease = useRef(false);

  const handleSidebarDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    sidebarDragging.current = true;
    sidebarMoved.current = false;
    collapseOnRelease.current = false;
    const startX = e.clientX;
    const startWidth = sidebarWidth;
    const startCollapsed = isSidebarCollapsed;
    if (!startCollapsed) {
      lastExpandedWidth.current = sidebarWidth;
    }
    const onMove = (ev: MouseEvent) => {
      if (!sidebarDragging.current) return;
      const delta = ev.clientX - startX;
      if (Math.abs(delta) > 3) {
        sidebarMoved.current = true;
      }
      const baseWidth = startCollapsed ? lastExpandedWidth.current : startWidth;
      const newWidth = Math.max(48, Math.min(400, baseWidth + delta));
      if (newWidth <= 56) {
        collapseOnRelease.current = true;
        setIsSidebarCollapsed(true);
        return;
      }
      collapseOnRelease.current = false;
      setIsSidebarCollapsed(false);
      setSidebarWidth(newWidth);
      lastExpandedWidth.current = newWidth;
    };
    const onUp = () => {
      sidebarDragging.current = false;
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      if (collapseOnRelease.current) onCollapse?.();
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, [isSidebarCollapsed, onCollapse, sidebarWidth]);

  const handleSidebarGrabClick = useCallback(() => {
    if (sidebarMoved.current) {
      sidebarMoved.current = false;
      return;
    }
    if (onCollapse) {
      onCollapse();
      return;
    }
    if (isSidebarCollapsed) {
      setSidebarWidth(lastExpandedWidth.current);
      setIsSidebarCollapsed(false);
    } else {
      lastExpandedWidth.current = sidebarWidth;
      setIsSidebarCollapsed(true);
    }
  }, [isSidebarCollapsed, onCollapse, sidebarWidth]);

  const sidebarRef = useRef<HTMLDivElement>(null);
  const touchMode = useTouchOptimizedUi();
  const sidebarActionText = pickUiFontScaleValue(uiFontScale, ["text-[10px]", "text-[11px]", "text-[12px]", "text-[13px]"]);
  const sidebarNavText = pickUiFontScaleValue(uiFontScale, ["text-[10px]", "text-[11px]", "text-[12px]", "text-[13px]"]);
  const sidebarNavTouchText = pickUiFontScaleValue(uiFontScale, ["text-[12px]", "text-[13px]", "text-[14px]", "text-[15px]"]);
  const scaledTouchSidebarWidth = Math.round(sidebarWidth * 1.3);
  const visibleSidebarWidth = isSidebarCollapsed
    ? 16
    : touchMode
      ? `min(85vw, ${scaledTouchSidebarWidth}px)`
      : sidebarWidth;
  const isCompact = !touchMode && !isSidebarCollapsed && sidebarWidth < 100;
  const prevTabIdsRef = useRef<Set<string>>(new Set());
  const [appearingTabs, setAppearingTabs] = useState<Set<string>>(new Set());
  const [closingTabs, setClosingTabs] = useState<Map<string, { wsId: string }>>(new Map());
  const allTabIds = useMemo(() => {
    const ids = new Set<string>();
    for (const ws of state.workspaces) {
      for (const tabId of ws.tabs) ids.add(tabId);
    }
    return ids;
  }, [state.workspaces]);

  useEffect(() => {
    const prev = prevTabIdsRef.current;
    if (prev.size > 0) {
      const newIds = new Set<string>();
      for (const id of allTabIds) {
        if (!prev.has(id)) newIds.add(id);
      }
      if (newIds.size > 0) {
        setAppearingTabs(newIds);
      }
    }
    prevTabIdsRef.current = allTabIds;
  }, [allTabIds]);
  useEffect(() => {
    if (appearingTabs.size === 0) return;
    const timer = setTimeout(() => setAppearingTabs(new Set()), 250);
    return () => clearTimeout(timer);
  }, [appearingTabs]);
  const [pendingTeamClose, setPendingTeamClose] = useState<{
    teamId: string;
    teamName: string;
    members: OrchestrationGroupMember[];
  } | null>(null);

  const performCloseTab = useCallback((wsId: string, tabId: string) => {
    setClosingTabs((prev) => {
      const next = new Map(prev);
      next.set(tabId, { wsId });
      return next;
    });
    setTimeout(() => {
      closeTab(wsId, tabId);
      setClosingTabs((prev) => {
        const next = new Map(prev);
        next.delete(tabId);
        return next;
      });
    }, 350);
  }, [closeTab]);

  const animatedCloseTab = useCallback((wsId: string, tabId: string) => {
    performCloseTab(wsId, tabId);
  }, [performCloseTab]);

  const requestTeamClose = useCallback((wsId: string, teamId: string) => {
    const ws = state.workspaces.find((workspace) => workspace.id === wsId);
    if (!ws) return;
    const teamTabs = tabsInWorkspace(ws.tabs, state.tabs).filter(
      (tab) => (tab as typeof tab & { team_id?: string | null }).team_id === teamId,
    );
    const team = state.teams.find((candidate) => candidate.id === teamId);
    setPendingTeamClose({
      teamId,
      teamName: team?.name?.trim() || "Team",
      members: teamTabs.map((tab) => {
        return {
          tabId: tab.id,
          title: tab.title || "Tab",
          role: tab.role ?? null,
          roleProfile: tab.role_profile ?? null,
        };
      }),
    });
  }, [state.tabs, state.teams, state.workspaces]);
  useEffect(() => {
    const kb = config?.keybindings ?? {};
    const handleKeyDown = (e: KeyboardEvent) => {
      const forward = matchesBinding(e, kb.next_tab ?? "Ctrl+Tab");
      const backward = !forward && matchesBinding(e, kb.prev_tab ?? "Ctrl+Shift+Tab");
      if (!forward && !backward) return;

      const ws = state.workspaces.find((w) => w.id === state.active_workspace_id);
      if (!ws) return;

      const items = buildSidebarCycleItems(ws.tabs, state.tabs, false);
      if (items.length < 2) return;

      const direction = forward ? 1 : -1;
      const next = cycleStep(items, state.activeTabIds[ws.id], workspaceCanvasActive, direction);
      if (!next || next.kind !== "tab") return;

      e.preventDefault();
      setActiveTab(ws.id, next.tabId);
      onNavigateToTab?.();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [config?.keybindings, state, workspaceCanvasActive, setActiveTab, onNavigateToTab]);

  const sortedWorkspaces = useMemo(() => sortByOrder(state.workspaces), [state.workspaces]);
  const { general: generalWorkspace, workspaces: ordinaryWorkspaces } = useMemo(
    () => partitionSidebarWorkspaces(sortedWorkspaces),
    [sortedWorkspaces],
  );
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string | null>(() => {
    const restored = loadSelectedWorkspaceId();
    if (restored) return restored;
    const active = state.workspaces.find((workspace) => workspace.id === state.active_workspace_id && !workspace.protected);
    return active?.id ?? state.workspaces.find((workspace) => !workspace.protected)?.id ?? null;
  });
  useEffect(() => {
    const active = ordinaryWorkspaces.find((workspace) => workspace.id === state.active_workspace_id);
    if (active) {
      setSelectedWorkspaceId(active.id);
      persistSelectedWorkspaceId(active.id);
      return;
    }
    if (!ordinaryWorkspaces.some((workspace) => workspace.id === selectedWorkspaceId)) {
      setSelectedWorkspaceId(ordinaryWorkspaces[0]?.id ?? null);
    }
  }, [ordinaryWorkspaces, selectedWorkspaceId, state.active_workspace_id]);
  const workspaceStatuses = useMemo(() => Object.fromEntries(ordinaryWorkspaces.map((workspace) => [
    workspace.id,
    aggregateTabStatuses(
      tabsInWorkspace(workspace.tabs, state.tabs)
        .filter(isSidebarTabVisible)
        .map((tab) => tab.id),
      state.tabStatuses,
    ),
  ])), [ordinaryWorkspaces, state.tabStatuses, state.tabs]);
  const workspaceSortIds = useMemo(
    () => ordinaryWorkspaces.map((ws) => `${WS_PREFIX}${ws.id}`),
    [ordinaryWorkspaces],
  );
  const dndHandlers = useMemo(
    () => ({ reorderTabs, reorderWorkspaces }),
    [reorderTabs, reorderWorkspaces],
  );

  return (
    <>
      <div
        className="relative overflow-visible shrink-0 h-full"
        style={{
          width: visibleSidebarWidth,
          transition: sidebarDragging.current ? undefined : "width 180ms cubic-bezier(0.16, 1, 0.3, 1)",
        }}
        >
        <aside
          ref={sidebarRef}
          id={SIDEBAR_REGION_ID}
          aria-label="Sidebar"
          data-testid="sidebar"
          data-touch-mode={touchMode ? "true" : "false"}
          className="shrink-0 h-full min-h-0 bg-[var(--ct-sidebar-bg)] border-r border-[var(--ct-border)] flex flex-col select-none overflow-x-hidden overflow-y-hidden relative"
          style={{ width: visibleSidebarWidth }}
        >
          {!isSidebarCollapsed && (
            <>
            <div className="ct-compact-scroll flex-1 min-w-0 overflow-y-auto overflow-x-hidden">
              {!isCompact && generalWorkspace && (() => {
                const generalExpanded = expandedWorkspaces.has(generalWorkspace.id);
                const generalActive = generalWorkspace.id === state.active_workspace_id && isWorkspaceMode;
                const generalTabs = tabsInWorkspace(generalWorkspace.tabs, state.tabs);
                const generalOrchestrators = Object.values(state.tabs).filter((tab) => anchorsTeam(tab));
                return (
                  <section
                    data-testid="general-section"
                    className="sidebar-general-section"
                    role="region"
                    aria-label={`${generalWorkspace.name} workspace contents`}
                  >
                    <div className="sidebar-structural-header w-full">
                      <SidebarDisclosure
                        data-testid="general-section-toggle"
                        className="sidebar-structural-toggle min-w-0"
                        expanded={generalExpanded}
                        onClick={() => toggleExpanded(generalWorkspace.id)}
                      >
                        <span className="sidebar-structural-label">GENERAL</span>
                      </SidebarDisclosure>
                      {IS_WEB && <WebPeerPicker compact />}
                    </div>
                    {generalExpanded && <SidebarWorkspaceTabs
                      workspace={generalWorkspace}
                      activeTabId={state.activeTabIds[generalWorkspace.id] ?? null}
                      sortedTabs={generalTabs}
                      tabs={state.tabs}
                      tabStatuses={state.tabStatuses}
                      declaredAttentionByTab={declaredAttention}
                      teams={state.teams}
                      planIndex={planIndex}
                      plans={fullPlans.plans}
                      activeWork={activeWork}
                      providers={visibleProviders}
                      providersLoading={providersLoading}
                      closingTabIds={new Set(closingTabs.keys())}
                      appearingTabIds={appearingTabs}
                      isActiveVisual={generalActive}
                      workspaceCanvasActive={workspaceCanvasActive}
                      touchMode={touchMode}
                      uiFontScale={uiFontScale}
                      compact={false}
                      sidebarActionText={sidebarActionText}
                      orchestrators={generalOrchestrators}
                      onBeforeOpenPlan={() => { setActiveWorkspace(generalWorkspace.id); onNavigateToTab?.(); }}
                      onActivateTab={(tabId) => { setActiveTab(generalWorkspace.id, tabId); onNavigateToTab?.(); }}
                      onCloseTab={(tabId) => animatedCloseTab(generalWorkspace.id, tabId)}
                      onRenameTab={(tabId, title) => setTabTitle(generalWorkspace.id, tabId, title)}
                      onNewSession={() => handleNewSession(generalWorkspace.id, generalWorkspace.cwd ?? null)}
                      onOpenRemoteTab={() => setRemotePickerWorkspaceId(generalWorkspace.id)}
                      newSessionDisabled={firstRunActive}
                      onRenameTeam={teamActions?.onRenameTeam}
                      onAddTeamMember={teamActions?.onAddTeamMember}
                      onCloseTeam={teamActions?.onCloseTeam
                        ? (teamId) => requestTeamClose(generalWorkspace.id, teamId)
                        : undefined}
                    />}
                  </section>
                );
              })()}
              <div className={`sidebar-structural-header ${isCompact ? "justify-center" : ""}`}>
                {!isCompact && <SidebarDisclosure
                  data-testid="workspaces-section-toggle"
                  className="sidebar-structural-toggle"
                  expanded={workspacesSectionExpanded}
                  onClick={toggleWorkspacesSection}
                >
                  <span className="sidebar-structural-label">WORKSPACES</span>
                </SidebarDisclosure>}
                <div className="sidebar-structural-action">
                  {isCompact && (
                    <button onClick={handleNewWorkspace} className="sidebar-new-workspace" title="New workspace" aria-label="Create workspace">
                      <FolderPlusIcon size={12} />
                    </button>
                  )}
                </div>
              </div>
              {workspaceSectionBodyVisible(isCompact, workspacesSectionExpanded) && (isCompact ? (
                <div className="flex flex-col items-center py-1">
                  {ordinaryWorkspaces.map((ws) => {
                    const isActive = ws.id === state.active_workspace_id;
                    const isActiveVisual = isActive && isWorkspaceMode;
                    const isExpanded = expandedWorkspaces.has(ws.id);
                    return (
                      <div key={ws.id} className="w-full flex flex-col items-center">
                        <button
                          onClick={() => { setActiveWorkspace(ws.id); toggleExpanded(ws.id); onNavigateToTab?.(); }}
                          className={`w-full py-1.5 flex items-center justify-center transition-colors ${
                            isActiveVisual ? "text-[var(--ct-fg)]" : "text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)]"
                          }`}
                          aria-label={`Workspace ${ws.name}`}
                          aria-expanded={isExpanded}
                          title={ws.name}
                        >
                          <FolderIcon size={12} open={isExpanded} className={isExpanded ? "opacity-70" : "opacity-40"} />
                        </button>
                        {isExpanded && (
                          <>
                            {flattenSidebarTabs(ws.tabs, state.tabs).map((tab) => {
                          const isTabActive = isWorkspaceTabActive(
                            isActiveVisual,
                            workspaceCanvasActive,
                            tab.id,
                            state.activeTabIds[ws.id],
                          );
                          return (
                            <button
                              key={tab.id}
                              onClick={() => { setActiveTab(ws.id, tab.id); onNavigateToTab?.(); }}
                              className={`w-8 h-7 flex items-center justify-center rounded transition-colors ${
                                isTabActive
                                  ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                                  : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-muted-fg)]"
                              }`}
                              aria-label={`Open tab ${displayTitleForTab(tab)}`}
                              title={displayTitleForTab(tab)}
                            >
                              {tab?.agent_type ? (
                                <AgentIcon providerId={tab.agent_type} className="text-[12px]" />
                              ) : (
                                <TerminalIcon size={11} />
                              )}
                            </button>
                          );
                            })}
                          </>
                        )}
                        <div className="w-5 h-px bg-[var(--ct-border)] my-0.5" />
                      </div>
                    );
                  })}
                </div>
              ) : (
                <SidebarDnd handlers={dndHandlers}>
                  <WorkspaceCardGrid
                    workspaces={ordinaryWorkspaces}
                    selectedWorkspaceId={selectedWorkspaceId}
                    statuses={workspaceStatuses}
                    sortIds={workspaceSortIds}
                    onSelect={(workspaceId) => {
                      setSelectedWorkspaceId(workspaceId);
                      persistSelectedWorkspaceId(workspaceId);
                      setActiveWorkspace(workspaceId);
                    }}
                    onCreate={handleNewWorkspace}
                  />
                  {ordinaryWorkspaces.map((ws) => {
                    const sortedTabs = tabsInWorkspace(ws.tabs, state.tabs);
                    const selected = ws.id === selectedWorkspaceId;
                    const isActiveVisual = ws.id === state.active_workspace_id && isWorkspaceMode;
                    const wsOrchestrators = Object.values(state.tabs).filter((tab) => anchorsTeam(tab));
                    return (
                      <div
                        key={ws.id}
                        data-testid="workspace-detail"
                        data-workspace-id={ws.id}
                        role="region"
                        aria-label={`${ws.name} workspace contents`}
                        hidden={!selected}
                      >
                    <WorkspaceItem
                      workspace={ws}
                      isActive={isActiveVisual}
                      isExpanded
                      onClick={() => { setActiveWorkspace(ws.id); onNavigateToTab?.(); }}
                      onToggleExpand={() => {}}
                      onClose={() => requestWorkspaceClose(ws.id)}
                      onRename={(name) => renameWorkspace(ws.id, name)}
                      onNewTerminal={() => createTab(ws.id)}
                      onNewSession={() => handleNewSession(ws.id, ws.cwd ?? null)}
                      newSessionDisabled={firstRunActive}
                      onNewAgent={(agentId) => handleAgentSpawn(agentId, ws.id, ws.cwd ?? null)}
                      canClose={true}
                      status={workspaceStatuses[ws.id]}
                      showDisclosure={false}
                      detailHeader
                      touchMode={touchMode}
                      uiFontScale={uiFontScale}
                    />
                    <SidebarWorkspaceTabs
                          workspace={ws}
                          activeTabId={state.activeTabIds[ws.id] ?? null}
                          sortedTabs={sortedTabs}
                          tabs={state.tabs}
                          tabStatuses={state.tabStatuses}
                          declaredAttentionByTab={declaredAttention}
                          teams={state.teams}
                          planIndex={planIndex}
                          plans={fullPlans.plans}
                          activeWork={activeWork}
                          providers={visibleProviders}
                          providersLoading={providersLoading}
                          closingTabIds={new Set(closingTabs.keys())}
                          appearingTabIds={appearingTabs}
                          isActiveVisual={isActiveVisual}
                          workspaceCanvasActive={workspaceCanvasActive}
                          touchMode={touchMode}
                          uiFontScale={uiFontScale}
                          compact={isCompact}
                          sidebarActionText={sidebarActionText}
                          orchestrators={wsOrchestrators}
                          onBeforeOpenPlan={() => {
                            setActiveWorkspace(ws.id);
                            onNavigateToTab?.();
                          }}
                          onActivateTab={(tabId) => {
                            setActiveTab(ws.id, tabId);
                            onNavigateToTab?.();
                          }}
                          onCloseTab={(tabId) => animatedCloseTab(ws.id, tabId)}
                          onRenameTab={(tabId, title) => setTabTitle(ws.id, tabId, title)}
                          onNewSession={() => handleNewSession(ws.id, ws.cwd ?? null)}
                          onOpenRemoteTab={() => setRemotePickerWorkspaceId(ws.id)}
                          newSessionDisabled={firstRunActive}
                          onRenameTeam={teamActions?.onRenameTeam}
                          onAddTeamMember={teamActions?.onAddTeamMember}
                          onCloseTeam={teamActions?.onCloseTeam
                            ? (teamId) => requestTeamClose(ws.id, teamId)
                            : undefined}
                        />
                      </div>
                    );
                  })}
                </SidebarDnd>
              ))}
            </div>
            {remotePickerWorkspaceId && (
              <RemoteTabPicker
                onClose={() => setRemotePickerWorkspaceId(null)}
                onAttach={(peer, tab) => attachRemoteTab?.(remotePickerWorkspaceId, peer, tab)}
              />
            )}
            {!isCompact && (() => {
              const views = state.recently_closed ?? [];
              const hasOrphaned = views.some((v) => v.orphaned);
              return (
                <RecentlyClosed
                  views={views}
                  showWorkspace={state.workspaces.length > 1 || hasOrphaned}
                  onReopen={(wsId, idx, orphaned, tabId) => {
                    if (orphaned) { restoreOrphaned(tabId); }
                    else { reopenClosed(wsId, idx); }
                  }}
                  onReopenGroup={reopenGroup}
                  touchMode={touchMode}
                  uiFontScale={uiFontScale}
                />
              );
            })()}
            <div
              role="navigation"
              aria-label="Sidebar views"
              className={`sidebar-footer ${isCompact ? "px-1 py-2 flex flex-col items-center" : "flex flex-col"}`}
            >
              <button
                data-testid="nav-extensions"
                aria-current={mainView === "extensions" ? "page" : undefined}
                onClick={onOpenExtensions}
                className={`flex items-center ${isCompact ? "justify-center w-8 h-8" : `gap-2 w-full px-2 ${touchMode ? "min-h-[44px] py-2.5" : "py-1.5"}`} rounded-md transition-colors ${
                  mainView === "extensions"
                    ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                    : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
                }`}
                title="Plugins"
              >
                <ExtensionsIcon size={touchMode ? 18 : 14} className="shrink-0" />
                {!isCompact && <span className={touchMode ? sidebarNavTouchText : sidebarNavText}>Plugins</span>}
              </button>
              <button
                data-testid="nav-memory"
                aria-current={mainView === "memory" ? "page" : undefined}
                onClick={onOpenMemory}
                className={`flex items-center ${isCompact ? "justify-center w-8 h-8" : `gap-2 w-full px-2 ${touchMode ? "min-h-[44px] py-2.5" : "py-1.5"}`} rounded-md transition-colors ${
                  mainView === "memory"
                    ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                    : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
                }`}
                title="Memory Graph (Ctrl+Shift+M)"
              >
                <svg width={touchMode ? 18 : 14} height={touchMode ? 18 : 14} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="shrink-0">
                  <circle cx="12" cy="5" r="2" />
                  <circle cx="5" cy="19" r="2" />
                  <circle cx="19" cy="19" r="2" />
                  <circle cx="12" cy="13" r="2" />
                  <line x1="12" y1="7" x2="12" y2="11" />
                  <line x1="10.5" y1="14.5" x2="6.5" y2="17.5" />
                  <line x1="13.5" y1="14.5" x2="17.5" y2="17.5" />
                </svg>
                {!isCompact && <span className={touchMode ? sidebarNavTouchText : sidebarNavText}>Memory</span>}
              </button>
              <div className={`flex items-center ${isCompact ? "justify-center" : "gap-1 w-full"}`}>
                <button
                  data-testid="nav-mesh"
                  aria-current={mainView === "mesh" ? "page" : undefined}
                  onClick={onOpenMesh}
                  className={`flex items-center ${isCompact ? "justify-center w-8 h-8" : `gap-2 flex-1 min-w-0 px-2 ${touchMode ? "min-h-[44px] py-2.5" : "py-1.5"}`} rounded-md transition-colors ${
                    mainView === "mesh"
                      ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                      : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
                  }`}
                  title="Mesh"
                >
                  <svg width={touchMode ? 18 : 14} height={touchMode ? 18 : 14} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="shrink-0">
                    <circle cx="12" cy="12" r="2" />
                    <circle cx="4" cy="6" r="2" />
                    <circle cx="20" cy="6" r="2" />
                    <circle cx="4" cy="18" r="2" />
                    <circle cx="20" cy="18" r="2" />
                    <line x1="6" y1="6" x2="10.5" y2="11" />
                    <line x1="18" y1="6" x2="13.5" y2="11" />
                    <line x1="6" y1="18" x2="10.5" y2="13" />
                    <line x1="18" y1="18" x2="13.5" y2="13" />
                  </svg>
                  {!isCompact && <span className={touchMode ? sidebarNavTouchText : sidebarNavText}>Mesh</span>}
                </button>
                {!isCompact && (
                  <LiquidFlip
                    data-testid="mesh-presence-toggle"
                    aria-label="Mesh presence"
                    className="shrink-0 text-[10px]"
                    checked={meshPresence === "online"}
                    onCheckedChange={setMeshEnabled}
                  />
                )}
              </div>
            </div>
          </>
        )}
      </aside>
      <button
        type="button"
        aria-controls={SIDEBAR_REGION_ID}
        aria-expanded={!isSidebarCollapsed}
        onMouseDown={handleSidebarDragStart}
        onClick={handleSidebarGrabClick}
        className={`absolute top-1/2 z-20 flex -translate-y-1/2 cursor-col-resize items-center justify-center rounded-r !border border-l-0 !border-[var(--ct-border)] bg-[color-mix(in_srgb,var(--ct-surface)_78%,transparent)] text-[var(--ct-muted-fg)] shadow-sm transition-colors hover:border-[var(--ct-accent)] hover:bg-[var(--ct-surface)] hover:text-[var(--ct-fg)] shell-narrow:hidden ${isSidebarCollapsed ? "left-0 h-8 w-2.5" : "left-[100%] -ml-[2px] h-9 w-3"}`}
        title={isSidebarCollapsed ? "Expand sidebar" : "Resize sidebar; click to collapse"}
      >
        <span className="h-4 w-px rounded-full bg-current opacity-60" />
      </button>
    </div>

      {folderPicker && (
        <RemoteFolderPicker
          title={folderPicker.title}
          initialPath={folderPicker.initialPath}
          onCancel={() => setFolderPicker(null)}
          onSelect={(path) => {
            setFolderPicker(null);
            folderPicker.onSelect(path);
          }}
        />
      )}
      {sessionPicker && (
        <NewSessionPicker
          workspaceId={sessionPicker.workspaceId}
          workspaceCwd={sessionPicker.workspaceCwd}
          agentsConfig={agentsConfig}
          agentConfig={config?.agent}
          initialProviderId={sessionPicker.initialProviderId}
          onClose={() => setSessionPicker(null)}
        />
      )}

      {orchestrationModal && (
        <OrchestrationCreate
          providers={visibleProviders}
          onCancel={() => setOrchestrationModal(null)}
          onConfirm={handleOrchestrationConfirm}
        />
      )}

      {pendingTeamClose && (
        <GroupCloseConfirmDialog
          mode="team"
          orchestratorTitle={pendingTeamClose.teamName}
          members={pendingTeamClose.members}
          onCancel={() => setPendingTeamClose(null)}
          onConfirm={() => {
            void teamActions?.onCloseTeam?.(
              pendingTeamClose.teamId,
            );
            setPendingTeamClose(null);
          }}
        />
      )}
      {pendingWorkspaceClose && (
        <WorkspaceCloseConfirmDialog
          workspaceName={pendingWorkspaceClose.name}
          agentTitles={pendingWorkspaceClose.agentTitles}
          onCancel={() => setPendingWorkspaceClose(null)}
          onConfirm={() => {
            closeWorkspace(pendingWorkspaceClose.id, true);
            setPendingWorkspaceClose(null);
          }}
        />
      )}
    </>
  );
}
