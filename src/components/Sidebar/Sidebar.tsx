import { isConciergeTab } from "../../lib/tabInputPolicy";
import { createNoticePublisher } from "../../lib/notices";
import { firstRunLocked, firstRunSelection, firstRunSidebarTabs } from "../../lib/firstRunGate";
import { FirstRunControls } from "../AgentPicker/FirstRunControls";
import { useTeamClose, useTeamClosePreview } from "./useTeamClose";
import { isWorkspaceCwdRefusal, spawnErrorMessage } from "../../lib/spawnErrorMessage";
import { precheckFreeLimit } from "../billing/freeLimitGate";
import { isFreeLimitRefusal } from "../billing/freeLimitModel";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { nativeFolderDialog as open } from "../../lib/nativeBridge";
import { IS_WEB } from "../../lib/webApi";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { isPhoneViewport } from "../../hooks/useSidebarViewport";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer/hooks";
import { shareRecord } from "../../lib/sameJson";
import { selectSidebarState, sidebarStateEqual } from "./sidebarState";
import { getPlugin } from "../../lib/plugins/registry";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import type { AppConfig, Tab } from "../../lib/types";
import { anchorsTeam } from "../../lib/roleKind";
import { MemoryGraphIcon, MeshIcon, NeighborsIcon, SidebarNavButton } from "./SidebarNavButton";
import { SidebarStructuralSection } from "./SidebarStructuralSection";
import { SidebarNeighborsSection } from "../Neighbors/SidebarNeighborsSection";
import { useNeighborsSwitch } from "../Neighbors/neighborsSwitch";
import {
  keepPressFocusInPlace,
  SidebarDnd,
  sortByOrder,
  WS_PREFIX,
} from "./sortable";
import { RemoteFolderPicker } from "../shared/RemoteFolderPicker";
import { LiquidFlip } from "../shared/LiquidToggle";
import type { MeshPresence } from "./meshPresenceHeader";
import { flattenSidebarTabs, isGeneralSectionTab, isSidebarTabVisible, tabsInWorkspace } from "./workspaceTabs";
import { aggregateTabStatuses, countWorkingTabs } from "./tabStatuses";
import { usePlanIndexOptional, useSidebarPlansOptional } from "../Canvas/PlansProvider";
import { readTeamClosePlanMetadata } from "./teamClosePlanRead";
import { renamePlan } from "../Canvas/plansApi";
import { isWorkspaceTabActive } from "./sidebarSelection";
import { SidebarWorkspaceDetail, type SidebarWorkspaceDetailActions, type SidebarWorkspaceDetailData } from "./SidebarWorkspaceDetail";
import { SidebarGeneralSection } from "./SidebarGeneralSection";
import { SidebarScrollLayout } from "./SidebarScrollLayout";
import { SidebarSkeleton } from "./SidebarSkeleton";
import type { JobsTarget } from "../Jobs/JobsView";
import type { JobsOpenScope } from "../Jobs/jobsLayout";
import { WorkspaceCardGrid } from "./WorkspaceCardGrid";
import { WorkspaceBrowseSection } from "./WorkspaceBrowseSection";
import { usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import { groupSidebarWorkspaceMachines, selectSidebarWorkspaceView, sidebarWorkspaceKey, useSidebarMachineFilter } from "./sidebarMachineFilter";
import { SidebarCompactRail, type CompactRailGroup } from "./SidebarCompactRail";
import { loadSelectedWorkspaceId, persistSelectedWorkspaceId } from "./workspaceCardMetadata";
import type { SidebarTeamActions } from "./TeamUnit";
import { GroupCloseConfirmDialog } from "./GroupCloseConfirmDialog";
import { BlueprintOwnerCloseDialog } from "./BlueprintOwnerCloseDialog";
import { WorkspaceCloseConfirmDialog } from "./WorkspaceCloseConfirmDialog";
import { useRemoteWorkspaceClose } from "./useRemoteWorkspaceClose";
import { hydrateProtectedExpanded, initialSidebarExpanded, partitionSidebarWorkspaces } from "./sidebarSections";
import { openNewSessionPicker } from "../../lib/newSessionChoice";
import { deskWorkspaceMembership, observedGeneralViewers } from "../../store/multiplexer/pairedWorkspaceProjection";
import { tabAttentionByTab } from "./tabAttention";
import type { TeamCloseResource } from "./teamCloseResources";
import { readWorkspaceBlueprints } from "../Automation/blueprintOwner";
import { runningBlueprintNamesForOwner } from "../Automation/blueprintOwnerModel";
import {
  loadWorkspacesSectionExpanded,
  persistWorkspacesSectionExpanded,
  toggleWorkspacesSectionExpanded,
  workspaceSectionBodyVisible,
} from "./workspacesSectionModel";

import { useSidebarScrollAnchor } from "./useSidebarScrollAnchor";
import { usePrimedWorkspaceGraphs } from "../Jobs/useWorkspaceGraphs";

const SIDEBAR_REGION_ID = "codeterm-sidebar";
const SIDEBAR_COLLAPSED_WIDTH = 0;
const SIDEBAR_COMPACT_WIDTH = 64;
const SIDEBAR_COMPACT_SNAP = 150;
const SIDEBAR_COLLAPSE_BELOW = 48;
const SIDEBAR_DEFAULT_WIDTH = 280;
const SIDEBAR_MAX_WIDTH = 400;
const SIDEBAR_PEEK_DWELL_MS = 280;

interface SidebarProps {
  onCollapse?: () => void;
  onOpenMemory?: () => void;
  onOpenMesh?: () => void;
  onOpenNeighbors?: () => void;
  /** The Neighbor whose conversation is the current view, for its row's highlight. */
  activeNeighborChat?: string | null;
  onNavigateToTab?: () => void;
  mainView?: "workspace" | "settings" | "memory" | "mesh" | "neighbors" | "neighbor" | "git" | "extensions" | "analytics";
  config?: AppConfig;
  uiFontScale?: number;
  workspaceCanvasActive?: boolean;
  onOpenWorkspaceMap?: (workspaceId: string) => void;
  onOpenWorkspaceAutomation?: (workspaceId: string, target?: JobsTarget, scope?: JobsOpenScope) => void;
  teamActions?: Partial<SidebarTeamActions>;
  onCollapsedChange?: (collapsed: boolean) => void;
  /** Bumped by the titlebar burger to bring a collapsed sidebar back. */
  expandRequest?: number;
  restoring?: boolean;
  firstRunActive?: boolean;
}

// Memoized: App re-renders on every store change; the sidebar has its own selector.
export const Sidebar = memo(function Sidebar({
  onCollapse,
  onOpenMemory,
  onOpenMesh,
  onOpenNeighbors,
  activeNeighborChat = null,
  onNavigateToTab,
  mainView = "workspace",
  config,
  uiFontScale,
  workspaceCanvasActive = false,
  onOpenWorkspaceMap,
  onOpenWorkspaceAutomation,
  teamActions,
  onCollapsedChange,
  expandRequest = 0,
  restoring: restoringWorkspace = false,
  firstRunActive = firstRunLocked(config?.general_agent),
}: SidebarProps) {
  const {
    createWorkspace,
    setActiveWorkspace,
    closeWorkspace,
    renameWorkspace,
    reorderTabs,
    reorderDisplay,
    reorderWorkspaces,
    createTab,
    closeTab,
    setActiveTab,
    renameTab,
    spawnAgentTab,
    upsertToast,
  } = useMultiplexer();
  const { generals, peerWorkspaces, ready: pairedInventoryReady = true } = usePairedWorkspaceInventory();
  const restoring = restoringWorkspace || !pairedInventoryReady;
  const backendState = useMuxSelector(selectSidebarState, sidebarStateEqual);
  const machineFilter = useSidebarMachineFilter();
  const selectedState = firstRunSelection(firstRunActive, { ...backendState, focused_tab_id: null });
  const visibleTabs = useMemo(() => firstRunSidebarTabs(firstRunActive, backendState.tabs), [firstRunActive, backendState.tabs]);
  const state = { ...selectedState, tabs: visibleTabs };
  // Rows compare attention by identity, so a tab whose attention did not change keeps its previous entry.
  const attentionRef = useRef<ReturnType<typeof tabAttentionByTab>>({});
  const attention = useMemo(() => {
    attentionRef.current = shareRecord(attentionRef.current, tabAttentionByTab(state.tabs, state.tabStatuses, state.notifications));
    return attentionRef.current;
  }, [state.tabs, state.tabStatuses, state.notifications]);
  const isWorkspaceMode = mainView === "workspace";
  const [folderPicker, setFolderPicker] = useState<{
    title: string;
    initialPath?: string | null;
    onSelect: (path: string) => void;
  } | null>(null);
  const [pendingWorkspaceClose, setPendingWorkspaceClose] = useState<{
    id: string;
    name: string;
    agentTitles: string[];
    blueprints: string[];
    teams: TeamCloseResource[];
  } | null>(null);
  const [closedRemoteWorkspaceKeys, setClosedRemoteWorkspaceKeys] = useState<ReadonlySet<string>>(() => new Set());
  const { providers, loading: providersLoading } = useAgentProviders();
  const { index: planIndex, refetch: refetchPlanIndex } = usePlanIndexOptional();
  const fullPlans = useSidebarPlansOptional();
  const planApi = config?.api;
  const refetchFullPlans = fullPlans.refetch;
  const handleRenamePlan = useMemo(() => planApi ? async (planId: string, title: string) => {
    await renamePlan(planApi, planId, title);
    await Promise.all([refetchPlanIndex(), refetchFullPlans()]);
  } : undefined, [planApi, refetchPlanIndex, refetchFullPlans]);
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
    if (state.workspaces.find((workspace) => workspace.id === workspaceId)?.cwd_missing) return;
    if (precheckFreeLimit(["agent.tabs"])) return;
    const reportFailure = (error: unknown) => {
      if (isWorkspaceCwdRefusal(error) || isFreeLimitRefusal(error)) return;
      createNoticePublisher({ upsertToast })({ kind: "toast", id: `spawn:${workspaceId}`,
        tabId: null,
        workspaceId: workspaceId,
        title: "Session start failed",
        detail: spawnErrorMessage(error),
        severity: "Error" });
    };
    let folder: string | null = workspaceCwd;
    if (!folder) {
      const provider = getPlugin(agentId);
      if (IS_WEB) {
        setFolderPicker({
          title: `Project folder for ${provider?.meta.displayName ?? agentId}`,
          initialPath: workspaceCwd,
          onSelect: (path) => {
            void spawnAgentTab(workspaceId, agentId, path).catch(reportFailure);
          },
        });
        return;
      } else {
        try {
          folder = await open({
            directory: true,
            multiple: false,
            title: `Choose project folder for ${provider?.meta.displayName ?? agentId}`,
          });
        } catch (err) {
          console.error(`[agent-spawn] folder picker failed:`, err);
          return;
        }
      }
      if (!folder) return;
    }
    void spawnAgentTab(workspaceId, agentId, folder).catch(reportFailure);
  }, [spawnAgentTab, state.workspaces, upsertToast]);


  const handleNewSession = useCallback((workspaceId: string, workspaceCwd: string | null) => {
    if (firstRunActive) return;
    openNewSessionPicker({ workspaceId, workspaceCwd });
  }, [firstRunActive]);

  const requestLocalWorkspaceClose = useCallback(async (workspaceId: string) => {
    const workspace = state.workspaces.find((workspace) => workspace.id === workspaceId);
    if (!workspace) return;
    try {
      const inventory = await readTeamClosePlanMetadata(workspaceId, null);
      setPendingWorkspaceClose({ id: workspaceId, name: workspace.name, ...inventory });
    } catch (error) {
      createNoticePublisher({ upsertToast })({ kind: "toast", tabId: null,
        workspaceId,
        title: "Close inventory unavailable",
        detail: error instanceof Error ? error.message : String(error),
        severity: "Error" });
    }
  }, [upsertToast, state.workspaces]);

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
    if (precheckFreeLimit(["workspaces"])) return;
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

  const neighborsSwitch = useNeighborsSwitch(config);

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

  const [sidebarWidth, setSidebarWidth] = useState(SIDEBAR_DEFAULT_WIDTH);
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false);
  const lastExpandedWidth = useRef(SIDEBAR_DEFAULT_WIDTH);
  const sidebarDragging = useRef(false);
  const sidebarMoved = useRef(false);
  const [peeking, setPeeking] = useState(false);
  const peekTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const holdPeek = useCallback(() => {
    if (peekTimer.current) clearTimeout(peekTimer.current);
    peekTimer.current = null;
    setPeeking(true);
  }, []);
  const releasePeek = useCallback(() => {
    if (peekTimer.current) clearTimeout(peekTimer.current);
    peekTimer.current = setTimeout(() => setPeeking(false), 220);
  }, []);
  const [dropY, setDropY] = useState<number | null>(null);
  const edgeEnter = useCallback(() => {
    if (peekTimer.current) clearTimeout(peekTimer.current);
    peekTimer.current = setTimeout(() => {
      setDropY(null);
      setPeeking(true);
    }, SIDEBAR_PEEK_DWELL_MS);
  }, []);
  const edgeLeave = useCallback(() => {
    setDropY(null);
    if (peekTimer.current) clearTimeout(peekTimer.current);
    peekTimer.current = setTimeout(() => setPeeking(false), 220);
  }, []);
  useEffect(() => () => {
    if (peekTimer.current) clearTimeout(peekTimer.current);
  }, []);
  const onCollapsedChangeRef = useRef(onCollapsedChange);
  onCollapsedChangeRef.current = onCollapsedChange;
  useEffect(() => {
    onCollapsedChangeRef.current?.(isSidebarCollapsed);
  }, [isSidebarCollapsed]);
  useEffect(() => {
    if (expandRequest === 0) return;
    setPeeking(false);
    setSidebarWidth(lastExpandedWidth.current);
    setIsSidebarCollapsed(false);
  }, [expandRequest]);

  const handleSidebarDragStart = useCallback((e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    setPeeking(false);
    sidebarDragging.current = true;
    sidebarMoved.current = false;
    const startX = e.clientX;
    const startWidth = isSidebarCollapsed ? SIDEBAR_COLLAPSED_WIDTH : sidebarWidth;
    let width = startWidth;
    const onMove = (ev: PointerEvent) => {
      if (ev.pointerId !== e.pointerId) return;
      if (!sidebarDragging.current) return;
      const delta = ev.clientX - startX;
      if (Math.abs(delta) > 3) sidebarMoved.current = true;
      if (!sidebarMoved.current) return;
      width = Math.max(SIDEBAR_COLLAPSED_WIDTH, Math.min(SIDEBAR_MAX_WIDTH, startWidth + delta));
      setIsSidebarCollapsed(false);
      setSidebarWidth(width);
    };
    const onUp = (ev: PointerEvent) => {
      if (ev.pointerId !== e.pointerId) return;
      sidebarDragging.current = false;
      document.removeEventListener("pointermove", onMove);
      document.removeEventListener("pointerup", onUp);
      document.removeEventListener("pointercancel", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      if (!sidebarMoved.current) return;
      if (width < SIDEBAR_COLLAPSE_BELOW) {
        setIsSidebarCollapsed(true);
        if (isPhoneViewport()) onCollapse?.();
        return;
      }
      const settled = width < SIDEBAR_COMPACT_SNAP ? SIDEBAR_COMPACT_WIDTH : width;
      lastExpandedWidth.current = settled;
      setSidebarWidth(settled);
    };
    document.addEventListener("pointermove", onMove);
    document.addEventListener("pointerup", onUp);
    document.addEventListener("pointercancel", onUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, [isSidebarCollapsed, onCollapse, sidebarWidth]);

  const sidebarRef = useRef<HTMLDivElement>(null);
  const touchMode = useTouchOptimizedUi();
  const sidebarActionText = pickUiFontScaleValue(uiFontScale, ["text-[10px]", "text-[11px]", "text-[12px]", "text-[13px]"]);
  const sidebarNavText = pickUiFontScaleValue(uiFontScale, ["text-[10px]", "text-[11px]", "text-[12px]", "text-[13px]"]);
  const sidebarNavTouchText = pickUiFontScaleValue(uiFontScale, ["text-[12px]", "text-[13px]", "text-[14px]", "text-[15px]"]);
  const navIconSize = touchMode ? 18 : 14;
  const scaledTouchSidebarWidth = Math.round(sidebarWidth * 1.3);
  const visibleSidebarWidth = isSidebarCollapsed
    ? SIDEBAR_COLLAPSED_WIDTH
    : touchMode
      ? `min(85vw, ${scaledTouchSidebarWidth}px)`
      : sidebarWidth;
  const isCompact = !touchMode && !isSidebarCollapsed && sidebarWidth < SIDEBAR_COMPACT_SNAP;
  const navButtonLayout = { compact: isCompact, touchMode, labelClassName: touchMode ? sidebarNavTouchText : sidebarNavText };
  const isPeeking = isSidebarCollapsed && peeking && !touchMode;
  const peekWidth = sidebarWidth >= SIDEBAR_COMPACT_SNAP ? sidebarWidth : 240;
  const [closingTabs, setClosingTabs] = useState<Map<string, { wsId: string }>>(new Map());
  const { pending: pendingTeamClose, request: readTeamClosePreview, dismiss: dismissTeamClose } = useTeamClosePreview();
  const [pendingTabClose, setPendingTabClose] = useState<{ workspaceId: string; tabId: string; title: string; blueprints: string[] } | null>(null);

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
    void (async () => {
      let blueprints: Awaited<ReturnType<typeof readWorkspaceBlueprints>>;
      try {
        blueprints = await readWorkspaceBlueprints(wsId);
      } catch (error) {
        createNoticePublisher({ upsertToast })({ kind: "toast", tabId: tabId,
          workspaceId: wsId,
          title: "Close inventory unavailable",
          detail: error instanceof Error ? error.message : String(error),
          severity: "Error" });
        return;
      }
      const running = runningBlueprintNamesForOwner(blueprints, "tab", tabId);
      if (running.length > 0) {
        setPendingTabClose({ workspaceId: wsId, tabId, title: state.tabs[tabId]?.title || "Tab", blueprints: running });
        return;
      }
      performCloseTab(wsId, tabId);
    })();
  }, [upsertToast, performCloseTab, state.tabs]);

  const requestTeamClose = useCallback(async (workspaceId: string, teamId: string) => {
    const workspace = state.workspaces.find((candidate) => candidate.id === workspaceId);
    if (!workspace) return;
    const team = state.teams.find((candidate) => candidate.id === teamId);
    const members = flattenSidebarTabs(workspace.tabs, state.tabs)
      .filter((tab) => tab.team_id === teamId)
      .map((tab) => ({
        tabId: tab.id,
        title: tab.title || "Tab",
        role: tab.role ?? null,
        roleProfile: tab.role_profile ?? null,
      }));
    await readTeamClosePreview({ id: teamId, workspaceId, name: team?.name?.trim() || "Team", members });
  }, [readTeamClosePreview, state.tabs, state.teams, state.workspaces]);

  const { closingTeamIds, closeTeam: finishTeamClose } = useTeamClose(teamActions?.onCloseTeam, createNoticePublisher({ upsertToast }));
  const confirmTeamClose = useCallback(() => {
    if (pendingTeamClose?.phase !== "ready") return;
    void finishTeamClose(pendingTeamClose.id, pendingTeamClose.workspaceId, dismissTeamClose);
  }, [pendingTeamClose, finishTeamClose, dismissTeamClose]);
  const sortedWorkspaces = useMemo(() => sortByOrder(state.workspaces), [state.workspaces]);
  const { general: generalWorkspace, workspaces: ordinaryWorkspaces } = useMemo(() => {
    const { general, workspaces } = partitionSidebarWorkspaces(sortedWorkspaces);
    const membership = deskWorkspaceMembership(sortedWorkspaces, state.tabs, state.teams);
    return { general, workspaces: workspaces.map(workspace => ({
      ...workspace, tabs: (membership.get(workspace.id) ?? []).map(tab => tab.id),
    })) };
  }, [sortedWorkspaces, state.tabs, state.teams]);
  const [preferredWorkspaceKey, setPreferredWorkspaceKey] = useState<string | null>(() => {
    const workspaceId = loadSelectedWorkspaceId() ?? state.active_workspace_id;
    return workspaceId ? sidebarWorkspaceKey(null, workspaceId) : null;
  });
  const activeWorkspaceId = state.active_workspace_id;
  const activeTabId = isWorkspaceMode && !workspaceCanvasActive && activeWorkspaceId
    ? state.activeTabIds[activeWorkspaceId] ?? null : null;
  const activeOrdinaryWorkspaceId = ordinaryWorkspaces.find(workspace => workspace.id === activeWorkspaceId)?.id;
  useEffect(() => {
    if (activeOrdinaryWorkspaceId) setPreferredWorkspaceKey(sidebarWorkspaceKey(null, activeOrdinaryWorkspaceId));
  }, [activeOrdinaryWorkspaceId]);
  const localWorkspaceIds = useMemo(() => new Map(ordinaryWorkspaces.map(workspace =>
    [sidebarWorkspaceKey(null, workspace.id), workspace.id])), [ordinaryWorkspaces]);
  const workspaceCandidates = useMemo(() => [
    ...ordinaryWorkspaces.map(workspace => ({
      key: sidebarWorkspaceKey(null, workspace.id), peerId: null, workspace, observed: null, selected: workspace.id === activeOrdinaryWorkspaceId,
    })),
    ...(peerWorkspaces ?? []).filter(observed => !closedRemoteWorkspaceKeys.has(sidebarWorkspaceKey(observed.peerId, observed.workspace.id))).map(observed => ({
      key: sidebarWorkspaceKey(observed.peerId, observed.workspace.id),
      peerId: observed.peerId,
      workspace: { ...observed.workspace, tabs: [...(observed.tabIds ?? observed.tabs.map(tab => tab.viewerId))] },
      observed,
      selected: observed.selected,
    })),
  ], [ordinaryWorkspaces, peerWorkspaces, closedRemoteWorkspaceKeys, activeOrdinaryWorkspaceId]);
  const workspaceView = useMemo(() => selectSidebarWorkspaceView(workspaceCandidates, preferredWorkspaceKey, machineFilter),
    [workspaceCandidates, preferredWorkspaceKey, machineFilter]);
  const selectedWorkspace = workspaceView.selected?.workspace ?? null;
  const selectedWorkspaceKey = workspaceView.selected?.key ?? null;
  const observedWorkspace = workspaceView.selected?.observed;
  const selectedWorkspaceKeyRef = useRef(selectedWorkspaceKey);
  selectedWorkspaceKeyRef.current = selectedWorkspaceKey;
  const generalByPeerId = useMemo(() => new Map(generals.map(general => [general.peerId, general])), [generals]);
  const machineNameCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const general of generals) counts.set(general.name, (counts.get(general.name) ?? 0) + 1);
    return counts;
  }, [generals]);
  const machineNameForPeer = useCallback((peerId: string) => {
    const general = generalByPeerId.get(peerId);
    const name = general?.name ?? "Machine";
    return !general || (machineNameCounts.get(name) ?? 0) > 1 ? `${name} · ${peerId.slice(0, 8)}` : name;
  }, [generalByPeerId, machineNameCounts]);
  const availabilityForPeer = useCallback((peerId: string) => generalByPeerId.get(peerId)?.availability, [generalByPeerId]);
  const onRemoteWorkspaceClosed = useCallback((peerId: string, workspaceId: string) => {
    const key = sidebarWorkspaceKey(peerId, workspaceId);
    setClosedRemoteWorkspaceKeys(previous => new Set(previous).add(key));
    setPreferredWorkspaceKey(previous => previous === key ? null : previous);
  }, []);
  const remoteWorkspaceClose = useRemoteWorkspaceClose(availabilityForPeer, onRemoteWorkspaceClosed);
  useEffect(() => {
    const pending = remoteWorkspaceClose.pending;
    if (pending && (
      sidebarWorkspaceKey(pending.peerId, pending.workspaceId) !== selectedWorkspaceKey
      || availabilityForPeer(pending.peerId) !== "online"
    )) {
      remoteWorkspaceClose.cancel();
    }
  }, [remoteWorkspaceClose.pending, remoteWorkspaceClose.cancel, selectedWorkspaceKey, availabilityForPeer]);
  const peerWorkspaceKeys = useMemo(() => new Set((peerWorkspaces ?? []).map(workspace => sidebarWorkspaceKey(workspace.peerId, workspace.workspace.id))), [peerWorkspaces]);
  useEffect(() => {
    setClosedRemoteWorkspaceKeys(previous => {
      const next = new Set([...previous].filter(key => peerWorkspaceKeys.has(key)));
      return next.size === previous.size ? previous : next;
    });
  }, [peerWorkspaceKeys]);
  const contentScrollRef = useSidebarScrollAnchor(activeWorkspaceId, selectedWorkspaceKey);
  usePrimedWorkspaceGraphs(useMemo(() => ordinaryWorkspaces.map((workspace) => workspace.id), [ordinaryWorkspaces]));
  const workspaceActivity = useMemo(() => Object.fromEntries(workspaceView.visible.map(({ key, workspace, observed }) => {
    const tabs = observed
      ? observed.tabs.map(({ viewerId, tab }) => ({ ...tab, id: viewerId })).filter(isSidebarTabVisible)
      : tabsInWorkspace(workspace.tabs, state.tabs).filter(isSidebarTabVisible);
    const statuses = observed
      ? Object.fromEntries(observed.tabs.flatMap(({ viewerId, status }) => status ? [[viewerId, status]] : []))
      : state.tabStatuses;
    return [key, {
      status: aggregateTabStatuses(tabs.map(tab => tab.id), statuses,
        observed ? Object.fromEntries(tabs.map(tab => [tab.id, tab])) : state.tabs),
      working: countWorkingTabs(tabs, statuses) > 0,
    }];
  })), [workspaceView.visible, state.tabs, state.tabStatuses]);
  const workspaceStatuses = useMemo(() => Object.fromEntries(Object.entries(workspaceActivity).map(([key, activity]) => [key, activity.status])), [workspaceActivity]);
  const workspaceWorking = useMemo(() => Object.fromEntries(Object.entries(workspaceActivity).map(([key, activity]) => [key, activity.working])), [workspaceActivity]);
  const workspaceMachineGroups = useMemo(() => groupSidebarWorkspaceMachines(workspaceView.visible,
    machineFilter === "all" ? [null, ...generals.map(general => general.peerId)] : machineFilter === "local" ? [null] : []),
  [workspaceView.visible, machineFilter, generals]);
  const [expandedWorkspaceMachines, setExpandedWorkspaceMachines] = useState<ReadonlySet<string>>(() => new Set());
  const setWorkspaceMachineExpanded = useCallback((peerId: string | null, expanded: boolean) => {
    const key = JSON.stringify([peerId]);
    setExpandedWorkspaceMachines(previous => {
      if (previous.has(key) === expanded) return previous;
      const next = new Set(previous);
      if (expanded) next.add(key);
      else next.delete(key);
      return next;
    });
  }, []);
  const workspaceSortIds = useMemo(() => [...localWorkspaceIds.keys()].map(key => `${WS_PREFIX}${key}`), [localWorkspaceIds]);
  const orchestratorTabs = useMemo(() => Object.values(state.tabs).filter((tab) => anchorsTeam(tab)), [state.tabs]);
  const closingTabIds = useMemo(() => new Set(closingTabs.keys()), [closingTabs]);
  const workspaceDetailData = useMemo<SidebarWorkspaceDetailData>(() => ({
    tabs: state.tabs,
    tabStatuses: state.tabStatuses,
    attention,
    teams: state.teams,
    planIndex,
    plans: fullPlans.plans,
    providers: visibleProviders,
    providersLoading,
    closingTabIds,
    closingTeamIds,
    activeTabIds: state.activeTabIds,
    activeWorkspaceId: state.active_workspace_id,
    isWorkspaceMode,
    workspaceCanvasActive,
    touchMode,
    uiFontScale,
    compact: isCompact,
    sidebarActionText,
    orchestrators: orchestratorTabs,
    newSessionDisabled: false,
  }), [state.tabs, state.tabStatuses, attention, state.teams, planIndex, fullPlans.plans, visibleProviders,
    providersLoading, closingTabIds, closingTeamIds, state.activeTabIds, state.active_workspace_id,
    isWorkspaceMode, workspaceCanvasActive, touchMode, uiFontScale, isCompact, sidebarActionText, orchestratorTabs]);
  const requestSelectedWorkspaceClose = useCallback((workspaceId: string) => {
    if (!selectedWorkspace || selectedWorkspace.id !== workspaceId) return;
    if (!observedWorkspace) {
      void requestLocalWorkspaceClose(workspaceId);
      return;
    }
    const targetKey = selectedWorkspaceKey;
    const target = {
      peerId: observedWorkspace.peerId,
      workspaceId: selectedWorkspace.id,
      workspaceName: selectedWorkspace.name,
      machineName: machineNameForPeer(observedWorkspace.peerId),
      availability: generalByPeerId.get(observedWorkspace.peerId)?.availability ?? "offline" as const,
      protected: selectedWorkspace.protected === true,
    };
    void remoteWorkspaceClose.request(target, () => selectedWorkspaceKeyRef.current === targetKey).then(result => {
      if (result.kind === "ready" || result.kind === "stale") return;
      const detail = result.kind === "inventory_failed"
        ? `${target.machineName} · ${target.workspaceName}\n${result.detail}`
        : result.kind === "protected"
          ? `${target.workspaceName} on ${target.machineName} is protected.`
          : `${target.workspaceName} on ${target.machineName} cannot be inspected while the machine is offline.`;
      createNoticePublisher({ upsertToast })({ kind: "toast", id: `workspace-close-inventory:${target.peerId}:${target.workspaceId}`,
        tabId: null, workspaceId: target.workspaceId, title: result.kind === "offline" ? "Machine offline" : "Close inventory unavailable",
        detail, severity: "Error" });
    });
  }, [selectedWorkspace, observedWorkspace, selectedWorkspaceKey, requestLocalWorkspaceClose, machineNameForPeer,
    generalByPeerId, remoteWorkspaceClose.request, upsertToast]);
  const confirmRemoteWorkspaceClose = useCallback(async () => {
    const pending = remoteWorkspaceClose.pending;
    if (pending && sidebarWorkspaceKey(pending.peerId, pending.workspaceId) !== selectedWorkspaceKey) {
      remoteWorkspaceClose.cancel();
      return;
    }
    const result = await remoteWorkspaceClose.confirm();
    if (result.kind === "none") return;
    const target = result.target;
    const targetLabel = `${target.workspaceName} on ${target.machineName}`;
    const notification = result.kind === "closed"
      ? { title: "Workspace closed", detail: targetLabel, severity: "Info" as const }
      : result.kind === "closed_with_warning"
        ? { title: "Workspace closed with a cleanup warning", detail: `${targetLabel}\n${result.detail}`, severity: "Error" as const }
        : result.kind === "still_present"
          ? { title: "Workspace remains open", detail: `${targetLabel}\n${result.detail}`, severity: "Error" as const }
          : result.kind === "offline"
            ? { title: "Workspace close not sent", detail: `${targetLabel} is offline.`, severity: "Error" as const }
            : { title: "Close status could not be checked", detail: `${targetLabel}\n${result.detail}\nRefresh the machine before starting another close.`, severity: "Error" as const };
    createNoticePublisher({ upsertToast })({ kind: "toast", id: `workspace-close:${target.peerId}:${target.workspaceId}`,
      tabId: null, workspaceId: target.workspaceId, ...notification });
  }, [remoteWorkspaceClose.pending, remoteWorkspaceClose.cancel, remoteWorkspaceClose.confirm, selectedWorkspaceKey, upsertToast]);
  const workspaceDetailActions = useMemo<SidebarWorkspaceDetailActions>(() => ({
    activateWorkspace: (workspaceId) => { setActiveWorkspace(workspaceId); onNavigateToTab?.(); },
    activateTab: (workspaceId, tabId) => { setActiveTab(workspaceId, tabId); onNavigateToTab?.(); },
    closeTab: animatedCloseTab,
    renameTab,
    renameWorkspace,
    closeWorkspace: requestSelectedWorkspaceClose,
    newTerminal: (workspaceId, shell) => {
      void createTab(workspaceId, undefined, shell).then((created) => { if (created) onNavigateToTab?.(); });
    },
    newSession: handleNewSession,
    newAgent: handleAgentSpawn,
    openMap: onOpenWorkspaceMap,
    openAutomation: onOpenWorkspaceAutomation,
    renamePlan: handleRenamePlan,
    renameTeam: teamActions?.onRenameTeam,
    addTeamMember: teamActions?.onAddTeamMember,
    closeTeam: teamActions?.onCloseTeam ? requestTeamClose : undefined,
  }), [setActiveWorkspace, onNavigateToTab, setActiveTab, animatedCloseTab, renameTab, renameWorkspace, requestSelectedWorkspaceClose,
    createTab, handleNewSession, handleAgentSpawn, onOpenWorkspaceMap, onOpenWorkspaceAutomation, handleRenamePlan,
    teamActions?.onRenameTeam, teamActions?.onAddTeamMember, teamActions?.onCloseTeam, requestTeamClose]);
  const dndHandlers = useMemo(
    () => ({
      reorderTabs,
      reorderDisplay,
      reorderWorkspaces: (keys: string[]) => reorderWorkspaces(keys.flatMap(key => localWorkspaceIds.get(key) ?? [])),
    }),
    [reorderTabs, reorderDisplay, reorderWorkspaces, localWorkspaceIds],
  );
  const compactGroups = useMemo<CompactRailGroup[]>(() => {
    if (!isCompact) return [];
    const railGroup = (workspaceId: string, label: string, tabIds: readonly string[], keep: (tab: Tab) => boolean): CompactRailGroup => {
      const workspaceActive = workspaceId === state.active_workspace_id && isWorkspaceMode;
      const tabs = tabsInWorkspace(tabIds, state.tabs).filter((tab) => isSidebarTabVisible(tab) && !isConciergeTab(tab) && !tab.parent_tab_id && keep(tab));
      const active = tabs.find((tab) => isWorkspaceTabActive(workspaceActive, workspaceCanvasActive, tab.id, state.activeTabIds[workspaceId]));
      const activeTabId = workspaceActive && !workspaceCanvasActive ? state.activeTabIds[workspaceId] ?? active?.id ?? null : active?.id ?? null;
      return { workspaceId, label, tabs, activeTabId };
    };
    const groups: CompactRailGroup[] = [];
    if (generalWorkspace) {
      const group = railGroup(generalWorkspace.id, generalWorkspace.name, generalWorkspace.tabs, tab => !tab.remote_peer_id && isGeneralSectionTab(tab));
      const peers = [...generals].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true }) || a.peerId.localeCompare(b.peerId));
      const viewers = observedGeneralViewers(peers, state.tabs, generalWorkspace.id);
      group.tabs.push(...viewers.map(({ session, attached }) => attached ?? { ...session.tab, id: session.viewerId }));
      group.observed = new Map(viewers.map(({ peer, session, attached }) => [session.viewerId, {
        machine: { peerId: peer.peerId, name: peer.name, availability: peer.availability, cached: peer.cached && !attached },
        status: session.status,
      }]));
      groups.push(group);
    }
    if (observedWorkspace && selectedWorkspace) {
      const { workspace, peerId, tabs } = observedWorkspace;
      const visible = tabs.filter(({ viewerId, tab }) => !isConciergeTab(tab)
        && !groups.some(group => group.tabs.some(visibleTab => visibleTab.id === viewerId)));
      groups.push({
        workspaceId: workspace.id,
        label: workspace.name,
        wholeWorkspace: true,
        tabs: visible.map(({ viewerId, tab }) => state.tabs[viewerId] ?? { ...tab, id: viewerId }),
        activeTabId: state.active_workspace_id ? state.activeTabIds[state.active_workspace_id] ?? null : null,
        observed: new Map(visible.map(({ viewerId, status, peerId: sourcePeer }) => {
          const machine = generals.find(peer => peer.peerId === (sourcePeer ?? peerId));
          return [viewerId, {
            machine: { peerId: sourcePeer ?? peerId, name: machine?.name ?? "Machine", availability: machine?.availability ?? "offline" },
            status,
          }];
        })),
      });
    } else if (selectedWorkspace) {
      groups.push({ ...railGroup(selectedWorkspace.id, selectedWorkspace.name, selectedWorkspace.tabs,
        (tab) => anchorsTeam(tab) || Boolean(tab.remote_peer_id)), wholeWorkspace: true });
    }
    return groups;
  }, [generalWorkspace, generals, observedWorkspace, isCompact, isWorkspaceMode, selectedWorkspace, state.activeTabIds, state.active_workspace_id, state.tabs, workspaceCanvasActive]);

  return (
    <FirstRunControls locked={firstRunActive}>
      <div
        className="relative overflow-visible shrink-0 h-full"
        style={{
          width: visibleSidebarWidth,
          transition: sidebarDragging.current ? undefined : "width var(--ct-duration-liquid-toggle) var(--ct-ease-liquid-settle)",
        }}
        >
        <aside
          ref={sidebarRef}
          id={SIDEBAR_REGION_ID}
          aria-label="Sidebar"
          data-testid="sidebar"
          data-touch-mode={touchMode ? "true" : "false"}
          data-collapsed={isSidebarCollapsed && !isPeeking ? "true" : "false"}
          data-peek={isPeeking || undefined}
          className="ct-sidebar-panel shrink-0 h-full min-h-0 grid select-none overflow-x-hidden overflow-y-hidden relative"
          style={isPeeking ? { width: "100%", "--ct-sidebar-peek-width": `${peekWidth}px` } as React.CSSProperties : { width: "100%" }}
          onMouseEnter={isPeeking ? holdPeek : undefined}
          onMouseLeave={isPeeking ? releasePeek : undefined}
        >
          {(!isSidebarCollapsed || isPeeking) && (
            <SidebarScrollLayout scrollRef={contentScrollRef} activeTabId={activeTabId}>
            {scrollRef => <>
            <div ref={scrollRef} className="ct-sidebar-content min-w-0 overflow-y-scroll overflow-x-hidden" onMouseDown={keepPressFocusInPlace}>
              {restoring && <SidebarSkeleton compact={isCompact} />}
              {isCompact && !restoring && (
                <SidebarCompactRail
                  groups={compactGroups}
                  attentionByTab={attention}
                  tabStatuses={state.tabStatuses}
                  touchMode={touchMode}
                  onActivate={(workspaceId, tabId) => {
                    setActiveTab(workspaceId, tabId);
                    onNavigateToTab?.();
                  }}
                />
              )}
              {/* One drag context for the whole sidebar; groups keep every drop inside its own list. */}
              <SidebarDnd handlers={dndHandlers}>
              {!isCompact && !restoring && generalWorkspace && (
                <SidebarGeneralSection
                  workspace={generalWorkspace}
                  expanded={firstRunActive || expandedWorkspaces.has(generalWorkspace.id)}
                  onToggle={() => toggleExpanded(generalWorkspace.id)}
                  providerId={config?.general_agent.agent_provider}
                  api={config?.api}
                  data={workspaceDetailData}
                  actions={workspaceDetailActions}
                  workspaces={state.workspaces}
                  onOpenJobs={onOpenWorkspaceAutomation}
                />
              )}
              {!isCompact && !restoring && <SidebarStructuralSection label="WORKSPACES" testId="workspaces-section-toggle"
                firstRunMuted={firstRunActive}
                expanded={workspacesSectionExpanded} onToggle={toggleWorkspacesSection}>
                {workspaceSectionBodyVisible(isCompact, workspacesSectionExpanded) && <>
                  {workspaceMachineGroups.map((group, index) => {
                    const groupKey = JSON.stringify([group.peerId]);
                    const availability = group.peerId ? availabilityForPeer(group.peerId) ?? "offline" : "online";
                    const machineStatus = availability === "update-required" ? "update required"
                      : availability === "signed-out" ? "signed out" : availability;
                    return <WorkspaceCardGrid
                      key={index === 0 ? "workspace-grid-primary" : groupKey}
                      workspaces={group.workspaces.map(({ key, workspace }) => ({ ...workspace, id: key }))}
                      selectedWorkspaceId={selectedWorkspaceKey}
                      statuses={workspaceStatuses}
                      workingByWorkspace={workspaceWorking}
                      sortIds={group.peerId === null ? workspaceSortIds : []}
                      fullWorkspaceOrder={[...localWorkspaceIds.keys()]}
                      localWorkspaceIds={localWorkspaceIds}
                      machinePeerId={group.peerId}
                      machineName={group.peerId === null ? undefined : machineNameForPeer(group.peerId)}
                      machineStatus={machineStatus}
                      showMachineHeader={machineFilter === "all"}
                      showCreate={group.peerId === null}
                      expanded={expandedWorkspaceMachines.has(groupKey)}
                      onExpandedChange={expanded => setWorkspaceMachineExpanded(group.peerId, expanded)}
                      onSelect={(key) => {
                        setPreferredWorkspaceKey(key);
                        const localId = localWorkspaceIds.get(key);
                        if (localId) {
                          persistSelectedWorkspaceId(localId);
                          setActiveWorkspace(localId);
                        }
                      }}
                      onCreate={handleNewWorkspace}
                    />;
                  })}
                  {selectedWorkspace && (
                    <SidebarWorkspaceDetail
                      key={selectedWorkspaceKey}
                      workspace={selectedWorkspace}
                      status={selectedWorkspaceKey ? workspaceActivity[selectedWorkspaceKey]?.status ?? null : null}
                      data={workspaceDetailData}
                      actions={workspaceDetailActions}
                      allowObservedWorkspaceClose={Boolean(observedWorkspace
                        && generalByPeerId.get(observedWorkspace.peerId)?.availability === "online"
                        && !selectedWorkspace.protected)}
                      observedContent={observedWorkspace && <WorkspaceBrowseSection
                        workspace={observedWorkspace}
                        attachedTabs={state.tabs}
                        touchMode={touchMode}
                        onOpenCanvas={onNavigateToTab}
                        onRenameViewer={(tab, title) => {
                          if (tab.workspace_id) renameTab(tab.workspace_id, tab.id, title);
                        }}
                        selectedViewerId={state.active_workspace_id ? state.activeTabIds[state.active_workspace_id] ?? null : null}
                        onSelectViewer={tabId => {
                          setActiveTab(generalWorkspace?.id ?? "", tabId);
                          onNavigateToTab?.();
                        }}
                      />}
                    />
                  )}
                </>}
              </SidebarStructuralSection>}
              </SidebarDnd>
            </div>
            {!isCompact && !restoring && <SidebarNeighborsSection uiFontScale={uiFontScale} touchMode={touchMode} onNavigateToTab={onNavigateToTab} enabled={neighborsSwitch.enabled} activeChat={activeNeighborChat}
              activeTabId={activeTabId} />}
            <div
              role="navigation"
              aria-label="Sidebar views"
              data-first-run-muted={firstRunActive || undefined}
              className={`sidebar-footer ${firstRunActive ? "opacity-70" : ""} ${isCompact ? "px-1 py-2 flex flex-col items-center" : "flex flex-col"}`}
            >
              <SidebarNavButton
                testId="nav-memory"
                label="Memory"
                title="Memory Graph (Ctrl+Shift+M)"
                icon={<MemoryGraphIcon size={navIconSize} />}
                current={mainView === "memory"}
                onClick={onOpenMemory}
                {...navButtonLayout}
              />
              <div className={`flex items-center ${isCompact ? "justify-center" : "gap-1 w-full"}`}>
                <SidebarNavButton
                  testId="nav-neighbors"
                  label="Neighborhood"
                  icon={<NeighborsIcon size={navIconSize} />}
                  current={mainView === "neighbors"}
                  onClick={onOpenNeighbors}
                  grow
                  {...navButtonLayout}
                />
                {!isCompact && (
                  <LiquidFlip
                    data-testid="neighbors-toggle"
                    aria-label="Neighbors"
                    className="shrink-0 text-[10px]"
                    checked={neighborsSwitch.enabled}
                    onCheckedChange={neighborsSwitch.setEnabled}
                  />
                )}
              </div>
              <div className={`flex items-center ${isCompact ? "justify-center" : "gap-1 w-full"}`}>
                <SidebarNavButton
                  testId="nav-mesh"
                  label="Mesh"
                  icon={<MeshIcon size={navIconSize} />}
                  current={mainView === "mesh"}
                  onClick={onOpenMesh}
                  grow
                  {...navButtonLayout}
                />
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
            </>}
            </SidebarScrollLayout>
        )}
      </aside>
      <button
        type="button"
        aria-controls={SIDEBAR_REGION_ID}
        role="separator"
        aria-label="Resize sidebar"
        aria-orientation="vertical"
        aria-valuemin={SIDEBAR_COLLAPSED_WIDTH}
        aria-valuemax={SIDEBAR_MAX_WIDTH}
        aria-valuenow={isSidebarCollapsed ? SIDEBAR_COLLAPSED_WIDTH : sidebarWidth}
        onPointerDown={handleSidebarDragStart}
        onMouseEnter={isSidebarCollapsed ? edgeEnter : undefined}
        onMouseMove={isSidebarCollapsed ? (event) => setDropY(event.clientY - event.currentTarget.getBoundingClientRect().top) : undefined}
        onMouseLeave={isSidebarCollapsed ? edgeLeave : undefined}
        className={`ct-sidebar-grip shell-narrow:hidden ${isSidebarCollapsed ? "ct-sidebar-grip--collapsed" : ""}`}
        title="Resize sidebar"
        style={{ touchAction: "none" }}
      >
        {isSidebarCollapsed && dropY !== null && <span className="ct-sidebar-drop" style={{ top: dropY }} aria-hidden="true" />}
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

      {pendingTeamClose && (
        <GroupCloseConfirmDialog
          mode="team"
          orchestratorTitle={pendingTeamClose.name}
          members={pendingTeamClose.members}
          crons={pendingTeamClose.phase === "ready" ? pendingTeamClose.crons : undefined}
          blueprints={pendingTeamClose.phase === "ready" ? pendingTeamClose.blueprints : undefined}
          lanes={pendingTeamClose.phase === "ready" ? pendingTeamClose.lanes : undefined}
          loading={pendingTeamClose.phase === "loading"}
          error={pendingTeamClose.phase === "error" ? pendingTeamClose.error : undefined}
          onRetry={() => { void requestTeamClose(pendingTeamClose.workspaceId, pendingTeamClose.id); }}
          onCancel={dismissTeamClose}
          onConfirm={() => { void confirmTeamClose(); }}
        />
      )}
      {pendingWorkspaceClose && (
        <WorkspaceCloseConfirmDialog
          workspaceName={pendingWorkspaceClose.name}
          agentTitles={pendingWorkspaceClose.agentTitles}
          blueprints={pendingWorkspaceClose.blueprints}
          teams={pendingWorkspaceClose.teams}
          onCancel={() => setPendingWorkspaceClose(null)}
          onConfirm={() => {
            const pending = pendingWorkspaceClose;
            setPendingWorkspaceClose(null);
            closeWorkspace(pending.id, true);
          }}
        />
      )}
      {remoteWorkspaceClose.pending && (
        <WorkspaceCloseConfirmDialog
          workspaceName={remoteWorkspaceClose.pending.workspaceName}
          machineName={remoteWorkspaceClose.pending.machineName}
          agentTitles={remoteWorkspaceClose.pending.agentTitles}
          crons={remoteWorkspaceClose.pending.crons}
          blueprints={remoteWorkspaceClose.pending.blueprints}
          teams={remoteWorkspaceClose.pending.teams}
          warnings={remoteWorkspaceClose.pending.warnings}
          onCancel={remoteWorkspaceClose.cancel}
          onConfirm={() => { void confirmRemoteWorkspaceClose(); }}
        />
      )}
      {pendingTabClose && (
        <BlueprintOwnerCloseDialog
          owner={`Tab ${pendingTabClose.title}`}
          blueprints={pendingTabClose.blueprints}
          onCancel={() => setPendingTabClose(null)}
          onConfirm={() => {
            const pending = pendingTabClose;
            setPendingTabClose(null);
            performCloseTab(pending.workspaceId, pending.tabId);
          }}
        />
      )}
    </FirstRunControls>
  );
});
