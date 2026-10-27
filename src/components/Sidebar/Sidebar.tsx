import { createNoticePublisher } from "../../lib/notices";
import { firstRunLocked, firstRunSelection, firstRunSidebarTabs } from "../../lib/firstRunGate";
import { FirstRunControls } from "../AgentPicker/FirstRunControls";
import { useTeamClose } from "./useTeamClose";
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
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import { shareRecord } from "../../lib/sameJson";
import { selectSidebarState, sidebarStateEqual } from "./sidebarState";
import { getAgentProvider } from "../../lib/agentProviders";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import type { AppConfig, CronJob, Tab } from "../../lib/types";
import { anchorsTeam } from "../../lib/roleKind";
import { MemoryGraphIcon, MeshIcon, NeighborsIcon, SidebarNavButton } from "./SidebarNavButton";
import { OrchestrationCreate, type OrchestrationCreateResult } from "./OrchestrationCreate";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { RecentlyClosed } from "./RecentlyClosed";
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
import { aggregateTabStatuses } from "./tabStatuses";
import { usePlanIndexOptional, useSidebarPlansOptional } from "../Canvas/PlansProvider";
import { renamePlan } from "../Canvas/plansApi";
import { isWorkspaceTabActive } from "./sidebarSelection";
import { SidebarWorkspaceDetail, type SidebarWorkspaceDetailActions, type SidebarWorkspaceDetailData } from "./SidebarWorkspaceDetail";
import { SidebarGeneralSection } from "./SidebarGeneralSection";
import { SidebarSkeleton } from "./SidebarSkeleton";
import { useSidebarScrollAnchor } from "./useSidebarScrollAnchor";
import type { JobsTarget } from "../Jobs/JobsView";
import type { JobsOpenScope } from "../Jobs/jobsLayout";
import { WorkspaceCardGrid } from "./WorkspaceCardGrid";
import { SidebarCompactRail, type CompactRailGroup } from "./SidebarCompactRail";
import { loadSelectedWorkspaceId, persistSelectedWorkspaceId } from "./workspaceCardMetadata";
import type { SidebarTeamActions } from "./TeamUnit";
import { GroupCloseConfirmDialog } from "./GroupCloseConfirmDialog";
import { BlueprintOwnerCloseDialog } from "./BlueprintOwnerCloseDialog";
import { WorkspaceCloseConfirmDialog } from "./WorkspaceCloseConfirmDialog";
import type { OrchestrationGroupMember } from "./orchestrationGroupClose";
import { hydrateProtectedExpanded, initialSidebarExpanded, partitionSidebarWorkspaces } from "./sidebarSections";
import { NewSessionPicker } from "../Terminal/NewSessionPicker";
import { OPEN_NEW_SESSION_EVENT, type OpenNewSessionDetail } from "../../lib/newSessionChoice";
import { RemoteTabPicker } from "./RemoteTabPicker";
import { tabAttentionByTab } from "./tabAttention";
import { teamCloseResourcesForId, teamCloseResourcesForWorkspace, type TeamCloseResource } from "./teamCloseResources";
import { readWorkspaceBlueprints } from "../Automation/blueprintOwner";
import { runningBlueprintNamesForOwner } from "../Automation/blueprintOwnerModel";
import {
  loadWorkspacesSectionExpanded,
  persistWorkspacesSectionExpanded,
  toggleWorkspacesSectionExpanded,
  workspaceSectionBodyVisible,
} from "./workspacesSectionModel";

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
  onNavigateToTab?: () => void;
  mainView?: "workspace" | "settings" | "memory" | "mesh" | "neighbors" | "git" | "extensions" | "analytics";
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
}

// Memoized: App re-renders on every store change; the sidebar has its own selector.
export const Sidebar = memo(function Sidebar({
  onCollapse,
  onOpenMemory,
  onOpenMesh,
  onOpenNeighbors,
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
  restoring = false,
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
    attachRemoteTab,
    closeTab,
    reopenClosed,
    reopenGroup,
    restoreOrphaned,
    setActiveTab,
    renameTab,
    spawnAgentTab,
    createOrchestrationPair,
    selectTabByTab,
    upsertToast,
  } = useMultiplexer();
  const backendState = useMuxSelector(selectSidebarState, sidebarStateEqual);
  const firstRunActive = firstRunLocked(config?.general_agent);
  const selectedState = firstRunSelection(firstRunActive, { ...backendState, focused_tab_id: null });
  const visibleTabs = useMemo(() => firstRunSidebarTabs(firstRunActive, backendState.tabs), [firstRunActive, backendState.tabs]);
  const state = { ...selectedState, tabs: visibleTabs };
  const reopenRecentlyClosed = useCallback((workspaceId: string, index: number, orphaned: boolean, tabId: string) => {
    if (orphaned) restoreOrphaned(tabId);
    else reopenClosed(workspaceId, index);
  }, [restoreOrphaned, reopenClosed]);
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
  const [orchestrationModal, setOrchestrationModal] = useState<{ workspaceId: string } | null>(null);
  const [pendingWorkspaceClose, setPendingWorkspaceClose] = useState<{
    id: string;
    name: string;
    agentTitles: string[];
    blueprints: string[];
    teams: TeamCloseResource[];
  } | null>(null);
  const [sessionPicker, setSessionPicker] = useState<OpenNewSessionDetail | null>(null);
  const [remotePickerWorkspaceId, setRemotePickerWorkspaceId] = useState<string | null>(null);
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
            void spawnAgentTab(workspaceId, agentId, path).catch(reportFailure);
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
    void spawnAgentTab(workspaceId, agentId, folder).catch(reportFailure);
  }, [config?.agent.full_access, spawnAgentTab, state.workspaces, upsertToast]);

  const handleOrchestrationConfirm = useCallback((result: OrchestrationCreateResult) => {
    if (!orchestrationModal) return;
    const workspaceId = orchestrationModal.workspaceId;
    setOrchestrationModal(null);
    createOrchestrationPair({ ...result, workspaceId }).then((created) => {
      void selectTabByTab(workspaceId, created.orchestrator_tab_id);
    }).catch((e) => {
      if (isFreeLimitRefusal(e)) return;
      createNoticePublisher({ upsertToast })({ kind: "toast", tabId: null,
        workspaceId: workspaceId,
        title: "Orchestration pair failed",
        detail: e instanceof Error ? e.message : String(e),
        severity: "Error" });
    });
  }, [upsertToast, createOrchestrationPair, orchestrationModal, selectTabByTab]);


  const handleNewSession = useCallback((workspaceId: string, workspaceCwd: string | null) => {
    if (firstRunActive) return;
    setSessionPicker({ workspaceId, workspaceCwd });
  }, [firstRunActive]);

  const requestWorkspaceClose = useCallback(async (workspaceId: string) => {
    const workspace = state.workspaces.find((workspace) => workspace.id === workspaceId);
    if (!workspace) return;
    let crons: CronJob[] = [];
    try {
      crons = await invoke<CronJob[]>("cron_list", {});
    } catch (error) {
      createNoticePublisher({ upsertToast })({ kind: "toast", tabId: null,
        workspaceId: workspaceId,
        title: "Close inventory unavailable",
        detail: error instanceof Error ? error.message : String(error),
        severity: "Error" });
    }
    let blueprintSnapshots: Awaited<ReturnType<typeof readWorkspaceBlueprints>>;
    try {
      blueprintSnapshots = await readWorkspaceBlueprints(workspaceId);
    } catch (error) {
      createNoticePublisher({ upsertToast })({ kind: "toast", tabId: null,
        workspaceId: workspaceId,
        title: "Close inventory unavailable",
        detail: error instanceof Error ? error.message : String(error),
        severity: "Error" });
      return;
    }
    const teams = teamCloseResourcesForWorkspace(
      workspace,
      state.tabs,
      state.teams ?? [],
      crons,
      fullPlans.plans,
      blueprintSnapshots,
    );
    const teamMemberIds = new Set(teams.flatMap((team) => team.members.map((member) => member.tabId)));
    const workspaceTabs = flattenSidebarTabs(workspace.tabs, state.tabs);
    const agentTitles = workspaceTabs
      .filter((tab) => tab.agent_type && !teamMemberIds.has(tab.id))
      .map((tab) => tab.title);
    const workspaceTabIds = new Set(workspaceTabs.map((tab) => tab.id));
    const blueprints = blueprintSnapshots.filter((blueprint) => blueprint.state === "running" && blueprint.owner?.kind === "tab" && blueprint.owner.tab_id !== null && workspaceTabIds.has(blueprint.owner.tab_id) && !teamMemberIds.has(blueprint.owner.tab_id)).map((blueprint) => blueprint.name);
    setPendingWorkspaceClose({ id: workspaceId, name: workspace.name, agentTitles, blueprints, teams });
  }, [upsertToast, fullPlans.plans, state.tabs, state.teams, state.workspaces]);

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
    workspaceId: string;
    teamName: string;
    members: OrchestrationGroupMember[];
    crons: string[];
    blueprints: string[];
    lanes: string[];
  } | null>(null);
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

  const requestTeamClose = useCallback(async (wsId: string, teamId: string) => {
    const ws = state.workspaces.find((workspace) => workspace.id === wsId);
    if (!ws) return;
    let crons: CronJob[] = [];
    try {
      crons = await invoke<CronJob[]>("cron_list", {});
    } catch (error) {
      createNoticePublisher({ upsertToast })({ kind: "toast", tabId: null,
        workspaceId: wsId,
        title: "Close inventory unavailable",
        detail: error instanceof Error ? error.message : String(error),
        severity: "Error" });
    }
    let blueprintSnapshots: Awaited<ReturnType<typeof readWorkspaceBlueprints>>;
    try {
      blueprintSnapshots = await readWorkspaceBlueprints(wsId);
    } catch (error) {
      createNoticePublisher({ upsertToast })({ kind: "toast", tabId: null,
        workspaceId: wsId,
        title: "Close inventory unavailable",
        detail: error instanceof Error ? error.message : String(error),
        severity: "Error" });
      return;
    }
    const resources = teamCloseResourcesForId(
      teamCloseResourcesForWorkspace(ws, state.tabs, state.teams ?? [], crons, fullPlans.plans, blueprintSnapshots),
      teamId,
    );
    const teamTabs = flattenSidebarTabs(ws.tabs, state.tabs).filter((tab) => tab.team_id === teamId);
    const team = state.teams.find((candidate) => candidate.id === teamId);
    setPendingTeamClose({
      teamId,
      workspaceId: wsId,
      teamName: team?.name?.trim() || "Team",
      members: resources?.members ?? teamTabs.map((tab) => {
        return {
          tabId: tab.id,
          title: tab.title || "Tab",
          role: tab.role ?? null,
          roleProfile: tab.role_profile ?? null,
        };
      }),
      crons: resources?.crons ?? [],
      blueprints: resources?.blueprints ?? runningBlueprintNamesForOwner(blueprintSnapshots, "team", teamId),
      lanes: resources?.lanes ?? [],
    });
  }, [upsertToast, fullPlans.plans, state.tabs, state.teams, state.workspaces]);

  const { closingTeamIds, closeTeam: finishTeamClose } = useTeamClose(teamActions?.onCloseTeam, createNoticePublisher({ upsertToast }));
  const confirmTeamClose = useCallback(() => {
    const pending = pendingTeamClose;
    if (!pending) return;
    void finishTeamClose(pending.teamId, pending.workspaceId, () => setPendingTeamClose(null));
  }, [pendingTeamClose, finishTeamClose]);
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
      state.tabs,
    ),
  ])), [ordinaryWorkspaces, state.tabStatuses, state.tabs]);
  const workspaceSortIds = useMemo(
    () => ordinaryWorkspaces.map((ws) => `${WS_PREFIX}${ws.id}`),
    [ordinaryWorkspaces],
  );
  const selectedWorkspace = ordinaryWorkspaces.find((workspace) => workspace.id === selectedWorkspaceId) ?? null;
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
    appearingTabIds: appearingTabs,
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
    providersLoading, closingTabIds, appearingTabs, closingTeamIds, state.activeTabIds, state.active_workspace_id,
    isWorkspaceMode, workspaceCanvasActive, touchMode, uiFontScale, isCompact, sidebarActionText, orchestratorTabs]);
  const workspaceDetailActions = useMemo<SidebarWorkspaceDetailActions>(() => ({
    activateWorkspace: (workspaceId) => { setActiveWorkspace(workspaceId); onNavigateToTab?.(); },
    activateTab: (workspaceId, tabId) => { setActiveTab(workspaceId, tabId); onNavigateToTab?.(); },
    closeTab: animatedCloseTab,
    renameTab,
    renameWorkspace,
    closeWorkspace: requestWorkspaceClose,
    newTerminal: (workspaceId, shell) => {
      void createTab(workspaceId, undefined, shell).then((created) => { if (created) onNavigateToTab?.(); });
    },
    newSession: handleNewSession,
    newAgent: handleAgentSpawn,
    openRemoteTab: setRemotePickerWorkspaceId,
    openMap: onOpenWorkspaceMap,
    openAutomation: onOpenWorkspaceAutomation,
    renamePlan: handleRenamePlan,
    renameTeam: teamActions?.onRenameTeam,
    addTeamMember: teamActions?.onAddTeamMember,
    closeTeam: teamActions?.onCloseTeam ? requestTeamClose : undefined,
  }), [setActiveWorkspace, onNavigateToTab, setActiveTab, animatedCloseTab, renameTab, renameWorkspace, requestWorkspaceClose,
    createTab, handleNewSession, handleAgentSpawn, onOpenWorkspaceMap, onOpenWorkspaceAutomation, handleRenamePlan,
    teamActions?.onRenameTeam, teamActions?.onAddTeamMember, teamActions?.onCloseTeam, requestTeamClose]);
  const dndHandlers = useMemo(
    () => ({ reorderTabs, reorderDisplay, reorderWorkspaces }),
    [reorderTabs, reorderDisplay, reorderWorkspaces],
  );
  const compactGroups = useMemo<CompactRailGroup[]>(() => {
    if (!isCompact) return [];
    const railGroup = (workspaceId: string, label: string, tabIds: readonly string[], keep: (tab: Tab) => boolean): CompactRailGroup => {
      const workspaceActive = workspaceId === state.active_workspace_id && isWorkspaceMode;
      const tabs = tabsInWorkspace(tabIds, state.tabs).filter((tab) => isSidebarTabVisible(tab) && !tab.parent_tab_id && keep(tab));
      const active = tabs.find((tab) => isWorkspaceTabActive(workspaceActive, workspaceCanvasActive, tab.id, state.activeTabIds[workspaceId]));
      return { workspaceId, label, tabs, activeTabId: active?.id ?? null };
    };
    const groups: CompactRailGroup[] = [];
    if (generalWorkspace) groups.push(railGroup(generalWorkspace.id, generalWorkspace.name, generalWorkspace.tabs, isGeneralSectionTab));
    const selected = ordinaryWorkspaces.find((workspace) => workspace.id === selectedWorkspaceId);
    if (selected) groups.push(railGroup(selected.id, selected.name, selected.tabs, (tab) => anchorsTeam(tab)));
    return groups;
  }, [generalWorkspace, isCompact, isWorkspaceMode, ordinaryWorkspaces, selectedWorkspaceId, state.activeTabIds, state.active_workspace_id, state.tabs, workspaceCanvasActive]);

  const scrollAnchor = useSidebarScrollAnchor(!isSidebarCollapsed || isPeeking);

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
          className="ct-sidebar-panel shrink-0 h-full min-h-0 flex flex-col select-none overflow-x-hidden overflow-y-hidden relative"
          style={isPeeking ? { width: "100%", "--ct-sidebar-peek-width": `${peekWidth}px` } as React.CSSProperties : { width: "100%" }}
          onMouseEnter={isPeeking ? holdPeek : undefined}
          onMouseLeave={isPeeking ? releasePeek : undefined}
        >
          {(!isSidebarCollapsed || isPeeking) && (
            <>
            <div ref={scrollAnchor.viewportRef} className="ct-sidebar-content flex-1 min-w-0 overflow-y-auto overflow-x-hidden" onMouseDown={keepPressFocusInPlace}>
              <div ref={scrollAnchor.bodyRef}>
              {restoring && <SidebarSkeleton compact={isCompact} />}
              {isCompact && !restoring && (
                <SidebarCompactRail
                  groups={compactGroups}
                  attentionByTab={attention}
                  tabStatuses={state.tabStatuses}
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
              {!isCompact && !restoring && <div className="sidebar-structural-section">
                <div className="sidebar-structural-header">
                  <SidebarDisclosure
                    data-testid="workspaces-section-toggle"
                    className="sidebar-structural-toggle"
                    expanded={workspacesSectionExpanded}
                    onClick={toggleWorkspacesSection}
                  >
                    <span className="sidebar-structural-label">WORKSPACES</span>
                  </SidebarDisclosure>
                  <div className="sidebar-structural-action" />
                </div>
                {workspaceSectionBodyVisible(isCompact, workspacesSectionExpanded) && (
                  <>
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
                    {selectedWorkspace && (
                      <SidebarWorkspaceDetail
                        key={selectedWorkspace.id}
                        workspace={selectedWorkspace}
                        status={workspaceStatuses[selectedWorkspace.id] ?? null}
                        data={workspaceDetailData}
                        actions={workspaceDetailActions}
                      />
                    )}
                  </>
                )}
              </div>}
              </SidebarDnd>
              </div>
              <div ref={scrollAnchor.spaceRef} aria-hidden="true" />
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
                  structures={state.closed_structures ?? []}
                  views={views}
                  showWorkspace={state.workspaces.length > 1 || hasOrphaned}
                  onReopen={reopenRecentlyClosed}
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
              <SidebarNavButton
                testId="nav-neighbors"
                label="Neighbors"
                icon={<NeighborsIcon size={navIconSize} />}
                current={mainView === "neighbors"}
                onClick={onOpenNeighbors}
                {...navButtonLayout}
              />
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
          </>
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
      {sessionPicker && (
        <NewSessionPicker
          workspaceId={sessionPicker.workspaceId}
          workspaceCwd={sessionPicker.workspaceCwd}
          agentsConfig={agentsConfig}
          agentConfig={config?.agent}
          initialProviderId={sessionPicker.initialProviderId}
          section={sessionPicker.section ?? (state.workspaces.find((workspace) => workspace.id === sessionPicker.workspaceId)?.protected ? "general" : "workspace")}
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
          crons={pendingTeamClose.crons}
          blueprints={pendingTeamClose.blueprints}
          lanes={pendingTeamClose.lanes}
          onCancel={() => setPendingTeamClose(null)}
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
