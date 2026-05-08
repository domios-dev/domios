import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useMultiplexer } from "../../store/multiplexer";
import { useCrons } from "../../store/crons";
import { getAgentProvider } from "../../lib/agentProviders";
import { TitleLevel } from "../../lib/types";
import type { Tab, SplitNode, PaneStatus, Pane } from "../../lib/types";
import { Plus } from "lucide-react";
import { TerminalIcon, FolderIcon, FolderPlusIcon, GearIcon } from "../Icons";
import { AgentIcon } from "../AgentIcon";
import { WorkspaceItem } from "./WorkspaceItem";
import { TabItem } from "./TabItem";
import { CronSection } from "./CronSection";

export { buildHierarchyTree, HierarchyTree } from "./HierarchyTree";

function collectPaneIdsRaw(node: SplitNode): string[] {
  if (node.type === "Leaf") return [node.pane_id];
  return node.children.flatMap(collectPaneIdsRaw);
}

function collectPaneIds(
  node: SplitNode,
  panes?: Record<string, Pane>,
): string[] {
  const ids = collectPaneIdsRaw(node);
  if (!panes) return ids;
  return ids.filter((id) => {
    const p = panes[id];
    return p && !p.hidden;
  });
}

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
  onOpenSettings?: () => void;
  onOpenMemory?: () => void;
  onOpenCron?: (jobId?: string) => void;
  onNavigateToTab?: () => void;
  mainView?: "workspace" | "settings" | "memory" | "cron";
  /** Cron job currently shown in the detail view of CronPanel — used to
   *  highlight the matching sidebar row with the same bg-[var(--ct-active)]
   *  treatment as Settings/Memory get. */
  activeCronId?: string;
}

export function Sidebar({
  onOpenSettings,
  onOpenMemory,
  onOpenCron,
  onNavigateToTab,
  mainView = "workspace",
  activeCronId,
}: SidebarProps) {
  const {
    state,
    createWorkspace,
    setActiveWorkspace,
    closeWorkspace,
    renameWorkspace,
    moveTab,
    createTab,
    closeTab,
    setActiveTab,
    setPaneTitle,
    spawnClaudeTab,
    spawnAgentTab,
  } = useMultiplexer();
  const { cronPaneIds } = useCrons();

  // When mainView leaves the workspace (Memory / Settings open), suppress the
  // "active" highlight on the current workspace + tab so the sidebar clearly
  // shows where the user actually is in the hierarchy. Backend focus state
  // is unchanged — returning to workspace restores the visual highlight.
  const isWorkspaceMode = mainView === "workspace";

  // Agent spawn modal state. `workspaceId` is captured up front so the
  // confirm step targets the workspace that opened the menu, not whatever
  // happens to be active when the user clicks Continue.
  const [agentSpawnModal, setAgentSpawnModal] = useState<
    { folder: string; agentId: string; workspaceId: string } | null
  >(null);
  const [claudeSkipPerms, setClaudeSkipPerms] = useState(() => {
    try { return localStorage.getItem("codeterm-claude-skip-perms") === "true"; } catch { return false; }
  });
  const [claudeRememberChoice, setClaudeRememberChoice] = useState(() => {
    try { return localStorage.getItem("codeterm-claude-remember") === "true"; } catch { return false; }
  });

  const handleAgentSpawn = useCallback(async (
    agentId: string,
    workspaceId: string,
    workspaceCwd: string | null,
  ) => {
    // Spawning from a workspace's 3-dot menu — the workspace folder is the
    // implicit choice; no need for a folder picker. Older versions opened a
    // dialog here even though the user had already selected the workspace.
    let folder: string | null = workspaceCwd;
    if (!folder) {
      const provider = getAgentProvider(agentId);
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
      if (!folder) return;
    }
    // If user chose "remember", skip the modal and spawn directly
    if (claudeRememberChoice) {
      spawnAgentTab(workspaceId, agentId, claudeSkipPerms, folder);
      return;
    }
    setAgentSpawnModal({ folder, agentId, workspaceId });
  }, [spawnAgentTab, claudeRememberChoice, claudeSkipPerms]);

  const handleClaudeConfirm = useCallback(() => {
    if (!agentSpawnModal) return;
    if (claudeRememberChoice) {
      try {
        localStorage.setItem("codeterm-claude-skip-perms", String(claudeSkipPerms));
        localStorage.setItem("codeterm-claude-remember", "true");
      } catch { /* ignore */ }
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
    try {
      folder = await open({ directory: true, multiple: false, title: "Choose workspace folder" });
    } catch {
      return;
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
  const sidebarDragging = useRef(false);

  const handleSidebarDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    sidebarDragging.current = true;
    const startX = e.clientX;
    const startWidth = sidebarWidth;
    const onMove = (ev: MouseEvent) => {
      if (!sidebarDragging.current) return;
      const newWidth = Math.max(48, Math.min(400, startWidth + ev.clientX - startX));
      setSidebarWidth(newWidth);
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
  }, [sidebarWidth]);

  const sidebarRef = useRef<HTMLDivElement>(null);
  const isCompact = sidebarWidth < 100;

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

  // Fire backend PTY-kill immediately so it runs in parallel with the visual
  // exit animation; defer the local state dispatch (which removes the tab)
  // until the 350ms animation completes so the pill doesn't disappear mid-fade.
  const animatedCloseTab = useCallback((wsId: string, tabId: string) => {
    const ws = state.workspaces.find((w) => w.id === wsId);
    const tab = ws?.tabs.find((t) => t.id === tabId);
    if (tab) {
      const collectPaneIdsLocal = (node: typeof tab.split_tree): string[] => {
        if (node.type === "Leaf") return [node.pane_id];
        return node.children.flatMap(collectPaneIdsLocal);
      };
      for (const pid of collectPaneIdsLocal(tab.split_tree)) {
        invoke("mux_close_pane", { paneId: pid }).catch(() => {});
      }
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

  return (
    <>
      <aside
        ref={sidebarRef}
        className="shrink-0 bg-[var(--ct-sidebar-bg)] flex flex-col select-none overflow-hidden relative"
        style={{ width: sidebarWidth, boxShadow: "inset -0.5px 0 0 var(--ct-border-subtle)" }}
      >
        {/* Panes header with action icons */}
        <div className={`flex items-center ${isCompact ? "justify-center" : "justify-between"} px-3 py-2`}>
          {!isCompact && (
            <span className="text-[10px] uppercase tracking-[0.06em] font-medium text-[var(--ct-muted)] truncate">
              Panes
            </span>
          )}
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

        {/* Workspace tree */}
        <div className="flex-1 overflow-y-auto">
          {isCompact ? (
            /* Compact mode — icon list with collapsible tabs */
            <div className="flex flex-col items-center py-1">
              {state.workspaces.map((ws) => {
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
                    {isExpanded && ws.tabs.map((tab) => {
                      const tabPaneIds = collectPaneIds(tab.split_tree, state.panes);
                      const isTabActive = isActiveVisual && tab.id === ws.active_tab_id;
                      return (
                        <button
                          key={tab.id}
                          onClick={() => { setActiveWorkspace(ws.id); setActiveTab(ws.id, tab.id); onNavigateToTab?.(); }}
                          className={`w-8 h-7 flex items-center justify-center rounded transition-colors ${
                            isTabActive
                              ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                              : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-muted-fg)]"
                          }`}
                          title={tab.title}
                        >
                          {(() => {
                            const agentPid = tabPaneIds.find((pid) => !!getAgentProvider(state.panes[pid]?.agent_type));
                            const tabProvider = agentPid ? getAgentProvider(state.panes[agentPid]?.agent_type) : undefined;
                            return tabProvider ? (
                              <AgentIcon providerId={tabProvider.id} className="text-[12px]" />
                            ) : (
                              <TerminalIcon size={11} />
                            );
                          })()}
                        </button>
                      );
                    })}
                    <div className="w-5 h-px bg-[var(--ct-border)] my-0.5" />
                  </div>
                );
              })}
            </div>
          ) : state.workspaces.map((ws) => {
            const allPaneIds = ws.tabs.flatMap((t) => collectPaneIds(t.split_tree, state.panes));
            const wsStatus = aggregateStatus(allPaneIds, state.paneStatuses);
            const hasWaiting = allPaneIds.some(
              (pid) => state.paneStatuses[pid] === "Waiting",
            );
            const isActive = ws.id === state.active_workspace_id;
            const isActiveVisual = isActive && isWorkspaceMode;
            const isExpanded = expandedWorkspaces.has(ws.id);
            // Orchestrators available for linking (across all workspaces)
            const wsOrchestrators = Object.values(state.panes).filter(
              (p) => p.role === "Orchestrator" || p.role === "Conductor" || p.role === "Lead" || (p.role === "Worker" && p.can_spawn),
            );

            return (
              <div key={ws.id}>
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
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={(e) => {
                    e.preventDefault();
                    try {
                      const data = JSON.parse(e.dataTransfer.getData("text/plain"));
                      if (data.tabId && data.fromWorkspaceId && data.fromWorkspaceId !== ws.id) {
                        moveTab(data.tabId, data.fromWorkspaceId, ws.id);
                      }
                    } catch { /* ignore */ }
                  }}
                />
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
                      // Build pane→tab lookup and find which tabs are children
                      const paneToTab: Record<string, typeof ws.tabs[0]> = {};
                      for (const tab of ws.tabs) {
                        const pids = collectPaneIds(tab.split_tree, state.panes);
                        if (pids[0]) paneToTab[pids[0]] = tab;
                      }
                      // Set of pane IDs that are children (have a parent) — skip at top level
                      const childPaneIds = new Set<string>();
                      for (const tab of ws.tabs) {
                        const pids = collectPaneIds(tab.split_tree, state.panes);
                        const pane = pids[0] ? state.panes[pids[0]] : null;
                        if (pane?.parent_pane_id) childPaneIds.add(pids[0]);
                      }

                      // Render a tab + its children recursively
                      const renderTab = (tab: Tab, index: number, depth: number): React.ReactNode => {
                        const tabPids = collectPaneIds(tab.split_tree, state.panes);
                        const firstPane = tabPids[0] ? state.panes[tabPids[0]] : null;
                        // Find child tabs (tabs whose pane.parent_pane_id === this pane)
                        const childTabs = firstPane
                          ? ws.tabs.filter((t) => {
                              const tPids = collectPaneIds(t.split_tree, state.panes);
                              const tPane = tPids[0] ? state.panes[tPids[0]] : null;
                              return tPane?.parent_pane_id === firstPane.id;
                            })
                          : [];

                        const isClosing = closingTabs.has(tab.id);
                        const isAppearing = appearingTabs.has(tab.id);

                        return (
                          <div key={tab.id} style={depth > 0 ? { paddingLeft: depth * 12 } : undefined}>
                            <TabItem
                              tab={tab}
                              index={index}
                              isActive={isActiveVisual && tab.id === ws.active_tab_id}
                              onClick={() => {
                                setActiveWorkspace(ws.id);
                                setActiveTab(ws.id, tab.id);
                                onNavigateToTab?.();
                              }}
                              onClose={() => animatedCloseTab(ws.id, tab.id)}
                              canClose={true}
                              paneCount={tabPids.length}
                              workspaceId={ws.id}
                              hasAgent={tabPids.some(
                                (pid) => !!getAgentProvider(state.panes[pid]?.agent_type)
                              )}
                              status={aggregateStatus(tabPids, state.paneStatuses)}
                              onRename={(title) => {
                                if (tabPids[0]) {
                                  setPaneTitle(tabPids[0], title, TitleLevel.User);
                                  invoke("mux_rename_pane", { paneId: tabPids[0], title }).then(() => {
                                  }).catch((_e) => {
                                  });
                                }
                              }}
                              pane={firstPane}
                              orchestrators={wsOrchestrators}
                              isAppearing={isAppearing}
                              isClosing={isClosing}
                            />
                            {childTabs.map((ct, ci) => renderTab(ct, ci, depth + 1))}
                          </div>
                        );
                      };

                      // Only render top-level tabs (not children, not cron-owned)
                      return ws.tabs
                        .filter((tab) => {
                          const pids = collectPaneIds(tab.split_tree, state.panes);
                          if (pids.length === 0) return false;
                          if (childPaneIds.has(pids[0])) return false;
                          if (pids.some((pid) => cronPaneIds.has(pid))) return false;
                          return true;
                        })
                        .map((tab, index) => renderTab(tab, index, 0));
                    })()}
                    {/* Add tab button */}
                    <button
                      onClick={async () => {
                        try {
                          const cfg = await invoke<{ agent: { resume_skip_permissions: boolean } }>("get_config");
                          const skip = cfg.agent.resume_skip_permissions;
                          spawnClaudeTab(ws.id, skip, ws.cwd ?? undefined);
                        } catch {
                          spawnClaudeTab(ws.id, false, ws.cwd ?? undefined);
                        }
                      }}
                      className="flex items-center gap-2 mx-2 mt-1 mb-0.5 px-5 py-1 rounded-md text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] transition-colors"
                      title="New Claude session in this workspace"
                    >
                      <Plus size={10} strokeWidth={1.5} />
                      <span className="text-[11px]">New session</span>
                    </button>
                    {/* Cron jobs section */}
                    <CronSection
                      workspaceId={ws.id}
                      onOpenCron={onOpenCron}
                      activeCronId={mainView === "cron" ? activeCronId : undefined}
                    />
                  </>
                  </div>
                </div>
                {!isExpanded && isActiveVisual && ws.active_tab_id ? (
                  /* Collapsed but active — show selected tab */
                  (() => {
                    const activeTab = ws.tabs.find((t) => t.id === ws.active_tab_id);
                    if (!activeTab) return null;
                    const atPids = collectPaneIds(activeTab.split_tree, state.panes);
                    const atFirstPane = atPids[0] ? state.panes[atPids[0]] : null;
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
                          (pid) => !!getAgentProvider(state.panes[pid]?.agent_type)
                        )}
                        status={aggregateStatus(atPids, state.paneStatuses)}
                        onRename={(title) => {
                          if (atPids[0]) {
                            setPaneTitle(atPids[0], title, TitleLevel.User);
                            invoke("mux_rename_pane", { paneId: atPids[0], title });
                          }
                        }}
                        pane={atFirstPane}
                        orchestrators={wsOrchestrators}
                      />
                    );
                  })()
                ) : null}
              </div>
            );
          })}
        </div>

        {/* Memory + Settings buttons at bottom */}
        <div className={`border-t border-[var(--ct-border)] ${isCompact ? "px-1 py-2 flex flex-col items-center gap-1" : "px-3 py-2 flex flex-col gap-1"}`}>
          <button
            onClick={onOpenMemory}
            className={`flex items-center ${isCompact ? "justify-center w-8 h-8" : "gap-2 w-full px-2 py-1.5"} rounded-md transition-colors ${
              mainView === "memory"
                ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
            }`}
            title="Memory Graph (Ctrl+Shift+M)"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="shrink-0">
              <circle cx="12" cy="5" r="2" />
              <circle cx="5" cy="19" r="2" />
              <circle cx="19" cy="19" r="2" />
              <circle cx="12" cy="13" r="2" />
              <line x1="12" y1="7" x2="12" y2="11" />
              <line x1="10.5" y1="14.5" x2="6.5" y2="17.5" />
              <line x1="13.5" y1="14.5" x2="17.5" y2="17.5" />
            </svg>
            {!isCompact && <span className="text-[11px]">Memory</span>}
          </button>
          <button
            onClick={onOpenSettings}
            className={`flex items-center ${isCompact ? "justify-center w-8 h-8" : "gap-2 w-full px-2 py-1.5"} rounded-md transition-colors ${
              mainView === "settings"
                ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
            }`}
            title="Settings (Ctrl+,)"
          >
            <GearIcon className="shrink-0" />
            {!isCompact && <span className="text-[11px]">Settings</span>}
          </button>
        </div>
        {/* Resize handle */}
        <div
          onMouseDown={handleSidebarDragStart}
          className="absolute right-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-[var(--ct-accent)] transition-colors"
        />
      </aside>

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
