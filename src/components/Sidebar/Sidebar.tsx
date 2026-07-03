import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { open } from "@tauri-apps/plugin-dialog";
import { IS_WEB } from "../../lib/webApi";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useMultiplexer } from "../../store/multiplexer";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { getAgentProvider } from "../../lib/agentProviders";
import { setProviderSettings } from "../../lib/clientSettings";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import type { Tab, PaneStatus, AppConfig } from "../../lib/types";
import { Plus } from "lucide-react";
import { TerminalIcon, FolderIcon, FolderPlusIcon, ExtensionsIcon } from "../Icons";
import { AgentIcon } from "../AgentIcon";
import { WorkspaceItem } from "./WorkspaceItem";
import { TabItem } from "./TabItem";
import { WorkspacePlanTreeSlot } from "./WorkspacePlanEntry";
import { RemoteWorkerRow } from "./RemoteWorkerRow";
import { RecentlyClosed } from "./RecentlyClosed";
import { useRemoteWorkers } from "../../hooks/useRemoteWorkers";
import {
  SidebarDnd,
  SortableRow,
  SortableContext,
  verticalListSortingStrategy,
  sortByOrder,
  WS_PREFIX,
  TAB_PREFIX,
} from "./sortable";
import { RemoteFolderPicker } from "../shared/RemoteFolderPicker";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import type { MeshPresence } from "./meshPresenceHeader";
import {
  childTabsForParent,
  collectPaneIds,
  flattenSidebarTabs,
  topLevelTabsInWorkspace,
} from "./workspaceTabs";
import { useFullPlansOptional, usePlanIndexOptional } from "../Canvas/PlansProvider";
import { resolvePaneTabActivity } from "../Canvas/paneTabActivity";
import { selectActiveWork } from "../Canvas/planView/activeWork";
import { buildHierarchySubtree, HierarchyTree } from "./HierarchyTree";

export { buildHierarchyTree, buildHierarchySubtree, HierarchyTree } from "./HierarchyTree";

/** Returns the "worst" status among a set of pane statuses. */
function aggregateStatus(
  paneIds: string[],
  statuses: Record<string, PaneStatus>,
): PaneStatus | null {
  const priority: PaneStatus[] = ["Dead", "Waiting", "Working", "Idle"];
  let worst: PaneStatus | null = null;
  for (const pid of paneIds) {
    const s = statuses[pid];
    if (!s) continue;
    if (!worst || priority.indexOf(s) < priority.indexOf(worst)) {
      worst = s;
    }
  }
  return worst;
}

interface SidebarProps {
  onOpenMemory?: () => void;
  onOpenMesh?: () => void;
  onOpenExtensions?: () => void;
  onNavigateToTab?: () => void;
  mainView?: "workspace" | "settings" | "memory" | "mesh" | "git" | "extensions";
  config?: AppConfig;
  uiFontScale?: number;
  workspaceCanvasActive?: boolean;
}

export function Sidebar({
  onOpenMemory,
  onOpenMesh,
  onOpenExtensions,
  onNavigateToTab,
  mainView = "workspace",
  config,
  uiFontScale,
  workspaceCanvasActive = false,
}: SidebarProps) {
  const {
    state,
    createWorkspace,
    setActiveWorkspace,
    closeWorkspace,
    renameWorkspace,
    reorderTabs,
    reorderWorkspaces,
    createTab,
    closeTab,
    reopenClosed,
    reopenGroup,
    restoreOrphaned,
    setActiveTab,
    setTabTitle,
    spawnAgentTab,
  } = useMultiplexer();
  // When mainView leaves the workspace (Memory / Settings open), suppress the "active" highlight on the current workspace + tab so the sidebar
  // clearly shows where the user is in the hierarchy. Backend focus state is unchanged — returning to workspace restores the visual highlight.
  const isWorkspaceMode = mainView === "workspace";

  // Agent spawn modal state. `workspaceId` is captured up front so the confirm step targets the workspace that opened the menu, not whatever
  // happens to be active when the user clicks Continue.
  const [agentSpawnModal, setAgentSpawnModal] = useState<
    { folder: string; agentId: string; workspaceId: string } | null
  >(null);
  const [folderPicker, setFolderPicker] = useState<{
    title: string;
    initialPath?: string | null;
    onSelect: (path: string) => void;
  } | null>(null);
  const [claudeSkipPerms, setClaudeSkipPerms] = useState(false);
  const [claudeRememberChoice, setClaudeRememberChoice] = useState(false);
  const { providers } = useAgentProviders();
  const { index: planIndex } = usePlanIndexOptional();
  const fullPlans = useFullPlansOptional();
  const activeWork = useMemo(
    () => selectActiveWork(fullPlans.plans, state.panes),
    [fullPlans.plans, state.panes],
  );
  const remoteWorkers = useRemoteWorkers();
  const agentsConfig = config?.agents;
  // Skip-permissions "remember" choice is centralized in backend config so it
  // carries across the desktop app and every web client (not per-localStorage).
  const claudeConfig = agentsConfig?.claude;
  const claudeSkipRemembered = claudeConfig?.spawn_skip_permissions_remembered === true;
  const claudeSavedSkipPerms = claudeConfig?.spawn_skip_permissions === true;
  const visibleProviders = useMemo(
    () => providers.filter((provider) => agentsConfig?.[provider.id]?.visible_in_menu !== false),
    [providers, agentsConfig],
  );

  const handleAgentSpawn = useCallback(async (
    agentId: string,
    workspaceId: string,
    workspaceCwd: string | null,
  ) => {
    // Spawning from a workspace's 3-dot menu — the workspace folder is the implicit choice; no need for a folder picker. Older versions opened a
    // dialog here even though the user had already selected the workspace.
    let folder: string | null = workspaceCwd;
    if (!folder) {
      const provider = getAgentProvider(agentId);
      if (IS_WEB) {
        setFolderPicker({
          title: `Project folder for ${provider?.displayName ?? agentId}`,
          initialPath: workspaceCwd,
          onSelect: (path) => {
            if (claudeSkipRemembered) {
              spawnAgentTab(workspaceId, agentId, claudeSavedSkipPerms, path);
            } else {
              setAgentSpawnModal({ folder: path, agentId, workspaceId });
            }
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
    // If user chose "remember", skip the modal and spawn directly
    if (claudeSkipRemembered) {
      spawnAgentTab(workspaceId, agentId, claudeSavedSkipPerms, folder);
      return;
    }
    setAgentSpawnModal({ folder, agentId, workspaceId });
  }, [spawnAgentTab, claudeSkipRemembered, claudeSavedSkipPerms]);

  const handleClaudeConfirm = useCallback(() => {
    if (!agentSpawnModal) return;
    if (claudeRememberChoice) {
      void setProviderSettings("claude", {
        spawn_skip_permissions: claudeSkipPerms,
        spawn_skip_permissions_remembered: true,
      });
    }
    spawnAgentTab(
      agentSpawnModal.workspaceId,
      agentSpawnModal.agentId,
      claudeSkipPerms,
      agentSpawnModal.folder,
    );
    setAgentSpawnModal(null);
  }, [agentSpawnModal, claudeSkipPerms, claudeRememberChoice, spawnAgentTab]);

  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Set<string>>(
    () => new Set(state.active_workspace_id ? [state.active_workspace_id] : []),
  );
  const [meshEnabledOverride, setMeshEnabledOverride] = useState<boolean | null>(null);

  // Auto-expand only the active workspace when it changes
  useEffect(() => {
    if (state.active_workspace_id) {
      setExpandedWorkspaces((prev) => {
        const next = new Set(prev);
        next.add(state.active_workspace_id!);
        return next;
      });
    }
  }, [state.active_workspace_id]);

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
    // useMemo auto-expands active workspace, but force expand all for immediate feedback
    setExpandedWorkspaces(prev => {
      const next = new Set(prev);
      state.workspaces.forEach(ws => next.add(ws.id));
      return next;
    });
  }, [createWorkspace, state.workspaces]);

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

  const [sidebarWidth, setSidebarWidth] = useState(220);
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false);
  const lastExpandedWidth = useRef(220);
  const sidebarDragging = useRef(false);
  const sidebarMoved = useRef(false);

  const handleSidebarDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    sidebarDragging.current = true;
    sidebarMoved.current = false;
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
        setIsSidebarCollapsed(true);
        return;
      }
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
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, [isSidebarCollapsed, sidebarWidth]);

  const handleSidebarGrabClick = useCallback(() => {
    if (sidebarMoved.current) {
      sidebarMoved.current = false;
      return;
    }
    if (isSidebarCollapsed) {
      setSidebarWidth(lastExpandedWidth.current);
      setIsSidebarCollapsed(false);
    } else {
      lastExpandedWidth.current = sidebarWidth;
      setIsSidebarCollapsed(true);
    }
  }, [isSidebarCollapsed, sidebarWidth]);

  const sidebarRef = useRef<HTMLDivElement>(null);
  const touchMode = useTouchOptimizedUi();
  const sidebarActionText = pickUiFontScaleValue(uiFontScale, ["text-[10px]", "text-[11px]", "text-[12px]", "text-[13px]"]);
  const sidebarNavText = pickUiFontScaleValue(uiFontScale, ["text-[10px]", "text-[11px]", "text-[12px]", "text-[13px]"]);
  const sidebarNavTouchText = pickUiFontScaleValue(uiFontScale, ["text-[12px]", "text-[13px]", "text-[14px]", "text-[15px]"]);
  const visibleSidebarWidth = isSidebarCollapsed ? 16 : touchMode ? Math.round(sidebarWidth * 1.3) : sidebarWidth;
  const isCompact = !touchMode && !isSidebarCollapsed && sidebarWidth < 100;

  // Track tabs for appear/close animations
  const prevTabIdsRef = useRef<Set<string>>(new Set());
  const [appearingTabs, setAppearingTabs] = useState<Set<string>>(new Set());
  const [closingTabs, setClosingTabs] = useState<Map<string, { wsId: string }>>(new Map());

  // Detect new tabs appearing
  const allTabIds = useMemo(() => {
    const ids = new Set<string>();
    for (const ws of state.workspaces) {
      for (const tab of ws.tabs) ids.add(tab.id);
    }
    return ids;
  }, [state.workspaces]);

  useEffect(() => {
    const prev = prevTabIdsRef.current;
    // Skip on first render (initial tabs should not animate in)
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

  // Clear appearing animation after delay — separate effect to avoid cleanup cancellation
  useEffect(() => {
    if (appearingTabs.size === 0) return;
    const timer = setTimeout(() => setAppearingTabs(new Set()), 250);
    return () => clearTimeout(timer);
  }, [appearingTabs]);

  // Fire backend PTY-kill immediately so it runs in parallel with the visual exit animation; defer the local state dispatch (which removes the
  // tab) until the 350ms animation completes so the pill doesn't disappear mid-fade.
  const animatedCloseTab = useCallback((wsId: string, tabId: string) => {
    const ws = state.workspaces.find((w) => w.id === wsId);
    const tab = ws?.tabs.find((t) => t.id === tabId);
    if (tab) {
      invoke("mux_close_pane", { paneId: tab.pane_id }).catch(() => {});
    }
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
  }, [closeTab, state.workspaces]);

  // Workspaces rendered in sort_order; the sidebar is the source of visual order.
  const sortedWorkspaces = useMemo(() => sortByOrder(state.workspaces), [state.workspaces]);
  const workspaceSortIds = useMemo(
    () => sortedWorkspaces.map((ws) => `${WS_PREFIX}${ws.id}`),
    [sortedWorkspaces],
  );
  // The General (protected) workspace is pinned to the top and never reorders.
  const pinnedWorkspaceIds = useMemo(
    () => sortedWorkspaces.filter((ws) => ws.protected).map((ws) => `${WS_PREFIX}${ws.id}`),
    [sortedWorkspaces],
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
          data-touch-mode={touchMode ? "true" : "false"}
          className="shrink-0 h-full min-h-0 bg-[var(--ct-sidebar-bg)] border-r border-[var(--ct-border)] flex flex-col select-none overflow-visible overflow-y-hidden relative"
          style={{ width: visibleSidebarWidth }}
        >
          {!isSidebarCollapsed && (
            <>
              {/* Header row — new-workspace action. Presence moved to the bottom nav. */}
              <div className={`flex items-center ${isCompact ? "justify-center" : "justify-end"} px-3 py-2`}>
                <div className="flex items-center gap-1">
                  {/* New workspace */}
                  <button
                    onClick={handleNewWorkspace}
                    className="flex items-center justify-center w-6 h-6 rounded border border-[var(--ct-border)] hover:bg-[var(--ct-hover)] hover:border-[var(--ct-muted)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
                    title="New workspace"
                  >
                    <FolderPlusIcon size={12} />
                  </button>
                </div>
              </div>

            <div className="flex-1 overflow-y-auto">
              {isCompact ? (
                /* Compact mode — icon list with collapsible tabs */
                <div className="flex flex-col items-center py-1">
                  {sortedWorkspaces.map((ws) => {
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
                          title={ws.name}
                        >
                          <FolderIcon size={12} open={isExpanded} className={isExpanded ? "opacity-70" : "opacity-40"} />
                        </button>
                        {isExpanded && (
                          <>
                            <WorkspacePlanTreeSlot
                              workspace={ws}
                              panes={state.panes}
                              planIndex={planIndex}
                              canvasActive={isActiveVisual && workspaceCanvasActive}
                              touchMode={touchMode}
                              compact={isCompact}
                              uiFontScale={uiFontScale}
                              onBeforeOpen={() => {
                                setActiveWorkspace(ws.id);
                                onNavigateToTab?.();
                              }}
                            />
                            {flattenSidebarTabs(ws.tabs, state.panes).map((tab) => {
                          const tabPaneIds = collectPaneIds(tab, state.panes);
                          const isTabActive = isActiveVisual && tab.id === state.activeTabIds[ws.id];
                          return (
                            <button
                              key={tab.id}
                              onClick={() => { setActiveTab(ws.id, tab.id); onNavigateToTab?.(); }}
                              className={`w-8 h-7 flex items-center justify-center rounded transition-colors ${
                                isTabActive
                                  ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                                  : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-muted-fg)]"
                              }`}
                              title={displayTitleForTab(tab, state.panes[tab.pane_id])}
                            >
                              {(() => {
                                // Any pane with an agent_type set (backend already classified) — don't gate on the FE-only `getAgentProvider` registry, or drop-in plugins
                                // without a FE module wouldn't surface here.
                                const agentPid = tabPaneIds.find((pid) => !!state.panes[pid]?.agent_type);
                                const agentType = agentPid ? state.panes[agentPid]?.agent_type : undefined;
                                return agentType ? (
                                  <AgentIcon providerId={agentType} className="text-[12px]" />
                                ) : (
                                  <TerminalIcon size={11} />
                                );
                              })()}
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
                <SortableContext items={workspaceSortIds} strategy={verticalListSortingStrategy}>
                {sortedWorkspaces.map((ws) => {
                const sortedTabs = sortByOrder(ws.tabs);
                const allPaneIds = sortedTabs.flatMap((t) => collectPaneIds(t, state.panes));
                const wsStatus = aggregateStatus(allPaneIds, state.paneStatuses);
                const hasWaiting = allPaneIds.some(
                  (pid) => state.paneStatuses[pid] === "Waiting",
                );
                const isActive = ws.id === state.active_workspace_id;
                const isActiveVisual = isActive && isWorkspaceMode;
                const isExpanded = expandedWorkspaces.has(ws.id);
                // Orchestrators available for linking (across all workspaces)
                const wsOrchestrators = Object.values(state.panes).filter(
                  (p) => p.role === "Orchestrator",
                );

                return (
                  <div key={ws.id}>
                    <SortableRow
                      id={`${WS_PREFIX}${ws.id}`}
                      disabled={ws.protected}
                      data={{ group: "workspace", groupKey: "workspaces", items: workspaceSortIds, pinnedIds: pinnedWorkspaceIds }}
                    >
                    <WorkspaceItem
                      workspace={ws}
                      isActive={isActiveVisual}
                      isExpanded={isExpanded}
                      onClick={() => { setActiveWorkspace(ws.id); onNavigateToTab?.(); }}
                      onToggleExpand={() => toggleExpanded(ws.id)}
                      onClose={() => closeWorkspace(ws.id)}
                      onRename={(name) => renameWorkspace(ws.id, name)}
                      onNewTerminal={() => createTab(ws.id)}
                      onNewAgent={(agentId) => handleAgentSpawn(agentId, ws.id, ws.cwd ?? null)}
                      canClose={true}
                      status={wsStatus}
                      hasWaiting={hasWaiting}
                      touchMode={touchMode}
                      uiFontScale={uiFontScale}
                    />
                    </SortableRow>
                    {/* Animated expand/collapse using grid-template-rows */}
                    <div
                      className="grid transition-[grid-template-rows] duration-[320ms]"
                      style={{
                        gridTemplateRows: isExpanded ? "1fr" : "0fr",
                        transitionTimingFunction: "cubic-bezier(0.16, 1, 0.3, 1)",
                      }}
                    >
                      <div className="overflow-hidden">
                        {/* Content always rendered — CSS grid-template-rows handles show/hide with animation */}
                        <>
                          {(() => {
                            // Render a tab + its children recursively. `group`/`groupKey`/
                            // `items` scope this tab's drag to its own sibling set.
                            const renderTab = (
                              tab: Tab,
                              index: number,
                              depth: number,
                              group: "tab" | "child",
                              groupKey: string,
                              items: string[],
                              pinnedIds: string[],
                            ): React.ReactNode => {
                              const tabPids = collectPaneIds(tab, state.panes);
                              const firstPane = tabPids[0] ? state.panes[tabPids[0]] : null;
                              const tabActivity = firstPane
                                ? resolvePaneTabActivity(planIndex, firstPane.id, state.panes)
                                : null;
                              const childTabs = firstPane
                                ? childTabsForParent(sortedTabs, state.panes, firstPane.id)
                                : [];
                              const childIds = childTabs.map((t) => `${TAB_PREFIX}${t.id}`);

                              const isClosing = closingTabs.has(tab.id);
                              const isAppearing = appearingTabs.has(tab.id);

                              return (
                                <div key={tab.id} style={depth > 0 ? { paddingLeft: depth * 12 } : undefined}>
                                  <SortableRow
                                    id={`${TAB_PREFIX}${tab.id}`}
                                    disabled={firstPane?.protected}
                                    data={{ group, groupKey, items, pinnedIds }}
                                  >
                                  <TabItem
                                    tab={tab}
                                    index={index}
                                    isActive={isActiveVisual && tab.id === state.activeTabIds[ws.id]}
                                    onClick={() => {
                                      setActiveTab(ws.id, tab.id);
                                      onNavigateToTab?.();
                                    }}
                                    onClose={() => animatedCloseTab(ws.id, tab.id)}
                                    canClose={true}
                                    paneCount={tabPids.length}
                                    workspaceId={ws.id}
                                    hasAgent={tabPids.some(
                                      (pid) => !!state.panes[pid]?.agent_type
                                    )}
                                    status={aggregateStatus(tabPids, state.paneStatuses)}
                                    onRename={(title) => {
                                      setTabTitle(ws.id, tab.id, title);
                                      invoke("mux_rename_tab", { workspaceId: ws.id, tabId: tab.id, title }).catch(() => {});
                                    }}
                                    pane={firstPane}
                                    orchestrators={wsOrchestrators}
                                    isAppearing={isAppearing}
                                    isClosing={isClosing}
                                    touchMode={touchMode}
                                    uiFontScale={uiFontScale}
                                    activity={tabActivity}
                                  />
                                  </SortableRow>
                                  {childTabs.length > 0 && firstPane && (
                                    firstPane.role === "Orchestrator" ? (
                                      <HierarchyTree
                                        nodes={buildHierarchySubtree(firstPane.id, state.panes, state.paneStatuses)}
                                        depth={depth + 1}
                                        onJumpToPane={(paneId) => {
                                          const childTab = sortedTabs.find((t) => t.pane_id === paneId);
                                          if (childTab) {
                                            setActiveTab(ws.id, childTab.id);
                                            onNavigateToTab?.();
                                          }
                                        }}
                                        reportCounts={{}}
                                        uiFontScale={uiFontScale}
                                        activeWork={activeWork}
                                        statuses={state.paneStatuses}
                                      />
                                    ) : (
                                      <SortableContext items={childIds} strategy={verticalListSortingStrategy}>
                                        {childTabs.map((ct, ci) =>
                                          renderTab(ct, ci, depth + 1, "child", firstPane.id, childIds, []),
                                        )}
                                      </SortableContext>
                                    )
                                  )}
                                  {/* Remote (cross-mesh) workers reporting to this pane — ghost nodes, no local PTY. */}
                                  {firstPane &&
                                    remoteWorkers[firstPane.id]?.map((w) => (
                                      <RemoteWorkerRow
                                        key={`${w.peer}/${w.remote_pane}`}
                                        worker={w}
                                        depth={depth + 1}
                                      />
                                    ))}
                                </div>
                              );
                            };

                            // Only render top-level tabs (children render under their parent).
                            const topLevel = topLevelTabsInWorkspace(sortedTabs, state.panes);
                            const topLevelIds = topLevel.map((t) => `${TAB_PREFIX}${t.id}`);
                            // Pin the General (protected) tab to the top of this group.
                            const pinnedTabIds = topLevel
                              .filter((t) => state.panes[t.pane_id]?.protected)
                              .map((t) => `${TAB_PREFIX}${t.id}`);
                            return (
                              <>
                                <WorkspacePlanTreeSlot
                                  workspace={ws}
                                  panes={state.panes}
                                  planIndex={planIndex}
                                  canvasActive={isActiveVisual && workspaceCanvasActive}
                                  touchMode={touchMode}
                                  compact={isCompact}
                                  uiFontScale={uiFontScale}
                                  onBeforeOpen={() => {
                                    setActiveWorkspace(ws.id);
                                    onNavigateToTab?.();
                                  }}
                                />
                                <SortableContext items={topLevelIds} strategy={verticalListSortingStrategy}>
                                  {topLevel.map((tab, index) =>
                                    renderTab(tab, index, 0, "tab", ws.id, topLevelIds, pinnedTabIds),
                                  )}
                                </SortableContext>
                              </>
                            );
                          })()}
                          {/* Add agent session button */}
                          {(() => {
                            const newSessionItems: DropdownMenuItem[] = visibleProviders.map((provider) => ({
                              label: `New ${provider.display_name} Session`,
                              icon: <AgentIcon providerId={provider.id} className="text-[12px]" />,
                              onClick: () => handleAgentSpawn(provider.id, ws.id, ws.cwd ?? null),
                            }));
                            const trigger = (
                              <button
                                disabled={newSessionItems.length === 0}
                                className="flex items-center gap-2 min-w-0 max-w-full mx-2 mt-1 mb-0.5 px-3 py-1 rounded-md text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] disabled:opacity-40 disabled:hover:bg-transparent transition-colors"
                                title={newSessionItems.length === 0 ? "No visible agent providers" : "New session in this workspace"}
                              >
                                <Plus size={10} strokeWidth={1.5} className="shrink-0" />
                                <span className={`${sidebarActionText} min-w-0 truncate`}>New session</span>
                              </button>
                            );
                            return newSessionItems.length === 0 ? (
                              trigger
                            ) : (
                              <DropdownMenu
                                items={newSessionItems}
                                trigger={trigger}
                              />
                            );
                          })()}
                        </>
                      </div>
                    </div>
                    {!isExpanded && isActiveVisual && state.activeTabIds[ws.id] ? (
                      /* Collapsed but active — show selected tab */
                      (() => {
                        const activeTab = ws.tabs.find((t) => t.id === state.activeTabIds[ws.id]);
                        if (!activeTab) return null;
                        const atPids = collectPaneIds(activeTab, state.panes);
                        const atFirstPane = atPids[0] ? state.panes[atPids[0]] : null;
                        const atActivity = atFirstPane
                          ? resolvePaneTabActivity(planIndex, atFirstPane.id, state.panes)
                          : null;
                        return (
                          <TabItem
                            tab={activeTab}
                            index={0}
                            isActive={isActiveVisual}
                            onClick={() => { setActiveTab(ws.id, activeTab.id); onNavigateToTab?.(); }}
                            onClose={() => animatedCloseTab(ws.id, activeTab.id)}
                            canClose={true}
                            paneCount={atPids.length}
                            workspaceId={ws.id}
                            hasAgent={atPids.some(
                              (pid) => !!state.panes[pid]?.agent_type
                            )}
                            status={aggregateStatus(atPids, state.paneStatuses)}
                            onRename={(title) => {
                              setTabTitle(ws.id, activeTab.id, title);
                              invoke("mux_rename_tab", { workspaceId: ws.id, tabId: activeTab.id, title }).catch(() => {});
                            }}
                            pane={atFirstPane}
                            orchestrators={wsOrchestrators}
                            touchMode={touchMode}
                            uiFontScale={uiFontScale}
                            activity={atActivity}
                          />
                        );
                      })()
                    ) : null}
                  </div>
                );
              })}
                </SortableContext>
                </SidebarDnd>
              )}
            </div>

            {/* Recently closed — pinned above bottom nav, aggregated across all workspaces */}
            {!isCompact && (() => {
              // Newest-first closed-history view, sourced from the sqlite store
              // (already bounded + sorted server-side).
              const views = state.recently_closed ?? [];
              if (views.length === 0) return null;
              const hasOrphaned = views.some((v) => v.orphaned);
              return (
                <RecentlyClosed
                  views={views}
                  showWorkspace={state.workspaces.length > 1 || hasOrphaned}
                  onReopen={(wsId, idx, orphaned, paneId) => {
                    if (orphaned) { restoreOrphaned(paneId); }
                    else { reopenClosed(wsId, idx); }
                  }}
                  onReopenGroup={reopenGroup}
                  touchMode={touchMode}
                  uiFontScale={uiFontScale}
                  headerTextClass={touchMode ? sidebarNavTouchText : sidebarNavText}
                />
              );
            })()}
            {/* Memory + Settings buttons at bottom */}
            <div className={`border-t border-[var(--ct-border)] mt-auto ${isCompact ? "px-1 py-2 flex flex-col items-center gap-1" : "px-3 py-2 flex flex-col gap-1"}`}>
              <button
                data-testid="nav-extensions"
                onClick={onOpenExtensions}
                className={`flex items-center ${isCompact ? "justify-center w-8 h-8" : `gap-2 w-full px-2 ${touchMode ? "py-2.5" : "py-1.5"}`} rounded-md transition-colors ${
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
                onClick={onOpenMemory}
                className={`flex items-center ${isCompact ? "justify-center w-8 h-8" : `gap-2 w-full px-2 ${touchMode ? "py-2.5" : "py-1.5"}`} rounded-md transition-colors ${
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
              {/* Mesh row — nav entry on the left, inline [off|on] segmented switch on the right. */}
              <div className={`flex items-center ${isCompact ? "justify-center" : "gap-1 w-full"}`}>
                <button
                  data-testid="nav-mesh"
                  onClick={onOpenMesh}
                  className={`flex items-center ${isCompact ? "justify-center w-8 h-8" : `gap-2 flex-1 min-w-0 px-2 ${touchMode ? "py-2.5" : "py-1.5"}`} rounded-md transition-colors ${
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
                  <div
                    data-testid="mesh-presence-toggle"
                    role="group"
                    aria-label="Mesh on/off"
                    className="flex shrink-0 items-center overflow-hidden rounded border border-[var(--ct-border)] text-[10px] font-medium"
                  >
                    <button
                      type="button"
                      data-testid="mesh-switch-off"
                      onClick={() => setMeshEnabled(false)}
                      aria-pressed={meshPresence !== "online"}
                      className={`px-2 py-0.5 transition-colors ${
                        meshPresence !== "online"
                          ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                          : "text-[var(--ct-muted)] hover:text-[var(--ct-fg)]"
                      }`}
                    >
                      off
                    </button>
                    <button
                      type="button"
                      data-testid="mesh-switch-on"
                      onClick={() => setMeshEnabled(true)}
                      aria-pressed={meshPresence === "online"}
                      className={`px-2 py-0.5 transition-colors border-l border-[var(--ct-border)] ${
                        meshPresence === "online"
                          ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                          : "text-[var(--ct-muted)] hover:text-[var(--ct-fg)]"
                      }`}
                    >
                      on
                    </button>
                  </div>
                )}
              </div>
            </div>
          </>
        )}
      </aside>

      {/* Resize/collapse grabber */}
      <button
        type="button"
        onMouseDown={handleSidebarDragStart}
        onClick={handleSidebarGrabClick}
        className={`absolute top-1/2 z-20 flex -translate-y-1/2 cursor-col-resize items-center justify-center rounded-r !border border-l-0 !border-[var(--ct-border)] bg-[color-mix(in_srgb,var(--ct-surface)_78%,transparent)] text-[var(--ct-muted-fg)] shadow-sm transition-colors hover:border-[var(--ct-accent)] hover:bg-[var(--ct-surface)] hover:text-[var(--ct-fg)] ${isSidebarCollapsed ? "left-0 h-8 w-2.5" : "left-[100%] -ml-[2px] h-9 w-3"}`}
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

      {/* Agent spawn modal */}
      {agentSpawnModal && (
        <>
          <div className="fixed inset-0 z-50 bg-black/40" onClick={() => setAgentSpawnModal(null)} />
          <div className="fixed z-50 top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-lg shadow-2xl w-[320px] overflow-hidden">
            <div className="px-4 pt-4 pb-2">
              <div className="text-[13px] font-medium text-[var(--ct-fg)]">Spawn {getAgentProvider(agentSpawnModal.agentId)?.displayName ?? agentSpawnModal.agentId}</div>
              <div className="mt-2 text-[11px] text-[var(--ct-muted)] truncate" title={agentSpawnModal.folder}>
                {agentSpawnModal.folder}
              </div>
            </div>
            <div className="px-4 py-3 flex flex-col gap-2.5">
              <label className="flex items-center gap-2.5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={claudeSkipPerms}
                  onChange={(e) => setClaudeSkipPerms(e.target.checked)}
                  className="w-3.5 h-3.5 rounded accent-[var(--ct-accent)]"
                />
                <span className="text-[12px] text-[var(--ct-fg)]">Skip permission prompts</span>
              </label>
              <label className="flex items-center gap-2.5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={claudeRememberChoice}
                  onChange={(e) => setClaudeRememberChoice(e.target.checked)}
                  className="w-3.5 h-3.5 rounded accent-[var(--ct-accent)]"
                />
                <span className="text-[12px] text-[var(--ct-muted-fg)]">Remember my choice</span>
              </label>
            </div>
            <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-[var(--ct-border)]">
              <button
                onClick={() => setAgentSpawnModal(null)}
                className="px-3 py-1.5 text-[11px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] rounded-md hover:bg-[var(--ct-hover)] transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleClaudeConfirm}
                className="px-4 py-1.5 text-[11px] font-medium text-white bg-[var(--ct-accent)] rounded-md hover:opacity-90 transition-opacity"
              >
                Launch
              </button>
            </div>
          </div>
        </>
      )}
    </>
  );
}
