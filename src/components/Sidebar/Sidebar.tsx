import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { nativeFolderDialog as open } from "../../lib/nativeBridge";
import { IS_WEB } from "../../lib/webApi";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { isPhoneViewport } from "../../hooks/useSidebarViewport";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import { selectSidebarState, sidebarStateEqual } from "./sidebarState";
import { getAgentProvider } from "../../lib/agentProviders";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import type { AppConfig, CronJob, Tab } from "../../lib/types";
import { anchorsTeam } from "../../lib/roleKind";
import { ExtensionsIcon } from "../Icons";
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
import { GeneralWorkspaceTabs } from "./GeneralWorkspaceTabs";
import type { JobsTarget } from "../Jobs/JobsView";
import type { JobsOpenScope } from "../Jobs/jobsLayout";
import { WorkspaceCardGrid } from "./WorkspaceCardGrid";
import { SidebarCompactRail, type CompactRailGroup } from "./SidebarCompactRail";
import { SpinalCordButton } from "./SpinalCord";
import { loadSelectedWorkspaceId, persistSelectedWorkspaceId } from "./workspaceCardMetadata";
import type { SidebarTeamActions } from "./TeamUnit";
import { GroupCloseConfirmDialog } from "./GroupCloseConfirmDialog";
import { BlueprintOwnerCloseDialog } from "./BlueprintOwnerCloseDialog";
import { WorkspaceCloseConfirmDialog } from "./WorkspaceCloseConfirmDialog";
import type { OrchestrationGroupMember } from "./orchestrationGroupClose";
import { hydrateProtectedExpanded, initialSidebarExpanded, partitionSidebarWorkspaces } from "./sidebarSections";
import { NewSessionPicker } from "../Terminal/NewSessionPicker";
import { OPEN_NEW_SESSION_EVENT, type OpenNewSessionDetail } from "../../lib/newSessionChoice";
import { isFirstRunGeneralTab } from "../AgentPicker";
import { RemoteTabPicker } from "./RemoteTabPicker";
import { tabAttentionByTab } from "./tabAttention";
import { teamCloseResourcesForId, teamCloseResourcesForWorkspace, type TeamCloseResource } from "./teamCloseResources";
import { readWorkspaceBlueprints } from "../Automation/blueprintOwner";
import { runningBlueprintNamesForOwner } from "../Automation/blueprintOwnerModel";


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
  onOpenExtensions?: () => void;
  onNavigateToTab?: () => void;
  mainView?: "workspace" | "settings" | "memory" | "mesh" | "git" | "extensions" | "analytics";
  config?: AppConfig;
  uiFontScale?: number;
  workspaceCanvasActive?: boolean;
  onOpenWorkspaceMap?: (workspaceId: string) => void;
  onOpenWorkspaceAutomation?: (workspaceId: string, target?: JobsTarget, scope?: JobsOpenScope) => void;
  teamActions?: Partial<SidebarTeamActions>;
  onCollapsedChange?: (collapsed: boolean) => void;
  /** Bumped by the titlebar burger to bring a collapsed sidebar back. */
  expandRequest?: number;
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
  onOpenWorkspaceMap,
  onOpenWorkspaceAutomation,
  teamActions,
  onCollapsedChange,
  expandRequest = 0,
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
    setTabTitle,
    spawnAgentTab,
    createOrchestrationPair,
    selectTabByTab,
    addNotification,
  } = useMultiplexer();
  const state = useMuxSelector(selectSidebarState, sidebarStateEqual);
  const attention = useMemo(
    () => tabAttentionByTab(state.tabs, state.tabStatuses, state.notifications),
    [state.tabs, state.tabStatuses, state.notifications],
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
    blueprints: string[];
    teams: TeamCloseResource[];
  } | null>(null);
  const [sessionPicker, setSessionPicker] = useState<OpenNewSessionDetail | null>(null);
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

  const requestWorkspaceClose = useCallback(async (workspaceId: string) => {
    const workspace = state.workspaces.find((workspace) => workspace.id === workspaceId);
    if (!workspace) return;
    let crons: CronJob[] = [];
    try {
      crons = await invoke<CronJob[]>("cron_list", {});
    } catch (error) {
      addNotification({
        tab_id: null,
        workspace_id: workspaceId,
        title: "Close inventory unavailable",
        body: error instanceof Error ? error.message : String(error),
        severity: "Error",
        level: "failure",
        sticky: true,
      });
    }
    let blueprintSnapshots: Awaited<ReturnType<typeof readWorkspaceBlueprints>>;
    try {
      blueprintSnapshots = await readWorkspaceBlueprints(workspaceId);
    } catch (error) {
      addNotification({ tab_id: null, workspace_id: workspaceId, title: "Close inventory unavailable", body: error instanceof Error ? error.message : String(error), severity: "Error", level: "failure", sticky: true });
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
  }, [addNotification, fullPlans.plans, state.tabs, state.teams, state.workspaces]);

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

  const handleSidebarDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setPeeking(false);
    sidebarDragging.current = true;
    sidebarMoved.current = false;
    const startX = e.clientX;
    const startWidth = isSidebarCollapsed ? SIDEBAR_COLLAPSED_WIDTH : sidebarWidth;
    let width = startWidth;
    const onMove = (ev: MouseEvent) => {
      if (!sidebarDragging.current) return;
      const delta = ev.clientX - startX;
      if (Math.abs(delta) > 3) sidebarMoved.current = true;
      if (!sidebarMoved.current) return;
      width = Math.max(SIDEBAR_COLLAPSED_WIDTH, Math.min(SIDEBAR_MAX_WIDTH, startWidth + delta));
      setIsSidebarCollapsed(false);
      setSidebarWidth(width);
    };
    const onUp = () => {
      sidebarDragging.current = false;
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
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
    if (onCollapse && isPhoneViewport()) {
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
    ? SIDEBAR_COLLAPSED_WIDTH
    : touchMode
      ? `min(85vw, ${scaledTouchSidebarWidth}px)`
      : sidebarWidth;
  const isCompact = !touchMode && !isSidebarCollapsed && sidebarWidth < SIDEBAR_COMPACT_SNAP;
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
        addNotification({ tab_id: tabId, workspace_id: wsId, title: "Close inventory unavailable", body: error instanceof Error ? error.message : String(error), severity: "Error", level: "failure", sticky: true });
        return;
      }
      const running = runningBlueprintNamesForOwner(blueprints, "tab", tabId);
      if (running.length > 0) {
        setPendingTabClose({ workspaceId: wsId, tabId, title: state.tabs[tabId]?.title || "Tab", blueprints: running });
        return;
      }
      performCloseTab(wsId, tabId);
    })();
  }, [addNotification, performCloseTab, state.tabs]);

  const requestTeamClose = useCallback(async (wsId: string, teamId: string) => {
    const ws = state.workspaces.find((workspace) => workspace.id === wsId);
    if (!ws) return;
    let crons: CronJob[] = [];
    try {
      crons = await invoke<CronJob[]>("cron_list", {});
    } catch (error) {
      addNotification({
        tab_id: null,
        workspace_id: wsId,
        title: "Close inventory unavailable",
        body: error instanceof Error ? error.message : String(error),
        severity: "Error",
        level: "failure",
        sticky: true,
      });
    }
    let blueprintSnapshots: Awaited<ReturnType<typeof readWorkspaceBlueprints>>;
    try {
      blueprintSnapshots = await readWorkspaceBlueprints(wsId);
    } catch (error) {
      addNotification({ tab_id: null, workspace_id: wsId, title: "Close inventory unavailable", body: error instanceof Error ? error.message : String(error), severity: "Error", level: "failure", sticky: true });
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
  }, [addNotification, fullPlans.plans, state.tabs, state.teams, state.workspaces]);

  const confirmTeamClose = useCallback(async () => {
    const pending = pendingTeamClose;
    const closeTeam = teamActions?.onCloseTeam;
    if (!pending || !closeTeam) return;
    try {
      const inventory = await closeTeam(pending.teamId);
      const kept = inventory.kept.map((lane) =>
        `${lane.path ?? "<no worktree>"} (${lane.branch ?? "no branch"}): ${lane.reason ?? "retained"}`,
      );
      const removed = inventory.removed.map((lane) =>
        `${lane.path ?? "<no worktree>"} (${lane.branch ?? "no branch"})`,
      );
      const orphanBranches = inventory.orphan_branches.join(", ");
      addNotification({
        tab_id: null,
        workspace_id: pending.workspaceId,
        title: kept.length > 0 ? "Team closed with lanes retained" : "Team closed",
        body: [
          `${inventory.removed.length} lane(s) removed.`,
          ...(removed.length > 0 ? [`Removed: ${removed.join("; ")}`] : []),
          ...(kept.length > 0 ? [`Kept: ${kept.join("; ")}`] : []),
          ...(orphanBranches ? [`Orphan branches: ${orphanBranches}`] : []),
        ].join(" "),
        severity: kept.length > 0 ? "Warning" : "Info",
        level: kept.length > 0 ? "action_needed" : "verbose",
        sticky: kept.length > 0,
      });
    } catch (error) {
      addNotification({
        tab_id: null,
        workspace_id: pending.workspaceId,
        title: "Team close failed",
        body: error instanceof Error ? error.message : String(error),
        severity: "Error",
        level: "failure",
        sticky: true,
      });
    } finally {
      setPendingTeamClose(null);
    }
  }, [addNotification, pendingTeamClose, teamActions?.onCloseTeam]);
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
      state.tabs,
    ),
  ])), [ordinaryWorkspaces, state.tabStatuses, state.tabs]);
  const workspaceSortIds = useMemo(
    () => ordinaryWorkspaces.map((ws) => `${WS_PREFIX}${ws.id}`),
    [ordinaryWorkspaces],
  );
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
    if (generalWorkspace) groups.push(railGroup(generalWorkspace.id, generalWorkspace.name, generalWorkspace.tabs, (tab) => Boolean(tab.agent_type)));
    const selected = ordinaryWorkspaces.find((workspace) => workspace.id === selectedWorkspaceId);
    if (selected) groups.push(railGroup(selected.id, selected.name, selected.tabs, (tab) => anchorsTeam(tab)));
    return groups;
  }, [generalWorkspace, isCompact, isWorkspaceMode, ordinaryWorkspaces, selectedWorkspaceId, state.activeTabIds, state.active_workspace_id, state.tabs, workspaceCanvasActive]);

  return (
    <>
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
            <div className="ct-sidebar-content flex-1 min-w-0 overflow-y-auto overflow-x-hidden">
              {isCompact && (
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
                    <div className="sidebar-structural-header sidebar-general-header w-full">
                      <SidebarDisclosure
                        data-testid="general-section-toggle"
                        className="sidebar-structural-toggle min-w-0"
                        expanded={generalExpanded}
                        onClick={() => toggleExpanded(generalWorkspace.id)}
                      >
                        <span className="sidebar-structural-label">GENERAL</span>
                      </SidebarDisclosure>
                      <SpinalCordButton api={config?.api} />
                    </div>
                    {generalExpanded && <GeneralWorkspaceTabs
                      workspace={generalWorkspace}
                      providerId={config?.general_agent.agent_provider}
                      generalCwd={generalWorkspace.cwd ?? generalTabs.find((tab) => tab.protected)?.cwd ?? null}
                      activeTabId={state.activeTabIds[generalWorkspace.id] ?? null}
                      sortedTabs={generalTabs}
                      tabs={state.tabs}
                      tabStatuses={state.tabStatuses}
                      tabAttentionByTab={attention}
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
                      onOpenRemoteTab={() => setRemotePickerWorkspaceId(generalWorkspace.id)}
                      newSessionDisabled={firstRunActive}
                      onRenameTeam={teamActions?.onRenameTeam}
                      onAddTeamMember={teamActions?.onAddTeamMember}
                      onCloseTeam={teamActions?.onCloseTeam
                        ? (teamId) => requestTeamClose(generalWorkspace.id, teamId)
                        : undefined}
                      onOpenSentinelJob={onOpenWorkspaceAutomation ? (target) => onOpenWorkspaceAutomation(generalWorkspace.id, target) : undefined}
                    />}
                  </section>
                );
              })()}
              {!isCompact && <div className="sidebar-structural-section">
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
                        onNewTerminal={(shell) => createTab(ws.id, undefined, shell)}
                        onNewSession={() => handleNewSession(ws.id, ws.cwd ?? null)}
                        onOpenMap={() => onOpenWorkspaceMap?.(ws.id)}
                        onOpenAutomation={() => onOpenWorkspaceAutomation?.(ws.id)}
                        newSessionDisabled={firstRunActive}
                        onNewAgent={(agentId) => handleAgentSpawn(agentId, ws.id, ws.cwd ?? null)}
                        canClose={true}
                        status={workspaceStatuses[ws.id]}
                        showDisclosure={false}
                        detailHeader
                        touchMode={touchMode}
                        countActiveJobs={selected}
                        uiFontScale={uiFontScale}
                      />
                      <SidebarWorkspaceTabs
                            workspace={ws}
                            activeTabId={state.activeTabIds[ws.id] ?? null}
                            sortedTabs={sortedTabs}
                            tabs={state.tabs}
                            tabStatuses={state.tabStatuses}
                            tabAttentionByTab={attention}
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
                            onOpenSentinelJob={onOpenWorkspaceAutomation ? (target) => onOpenWorkspaceAutomation(ws.id, target) : undefined}
                          />
                        </div>
                      );
                    })}
                  </SidebarDnd>
                )}
              </div>}
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
        onMouseEnter={isSidebarCollapsed ? edgeEnter : undefined}
        onMouseMove={isSidebarCollapsed ? (event) => setDropY(event.clientY - event.currentTarget.getBoundingClientRect().top) : undefined}
        onMouseLeave={isSidebarCollapsed ? edgeLeave : undefined}
        className={`ct-sidebar-grip shell-narrow:hidden ${isSidebarCollapsed ? "ct-sidebar-grip--collapsed" : ""}`}
        title={isSidebarCollapsed ? "Expand sidebar" : "Resize sidebar; click to collapse"}
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
            closeWorkspace(pendingWorkspaceClose.id, true);
            setPendingWorkspaceClose(null);
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
    </>
  );
}
