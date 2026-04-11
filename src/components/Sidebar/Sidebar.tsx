import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useMultiplexer } from "../../store/multiplexer";
import { useCrons } from "../../store/crons";
import { CronEditor } from "../CronEditor/CronEditor";
import { getAgentProvider, getAllProviders } from "../../lib/agentProviders";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { TitleLevel } from "../../lib/types";
import type { Workspace, Tab, SplitNode, PaneStatus, Pane, AgentRole, CronJob } from "../../lib/types";
import { X, Pencil, UserMinus, Ban, CircleCheck, TerminalSquare, Plus, FolderOpen } from "lucide-react";
import { TelegramIcon, TerminalIcon, FolderIcon, FolderPlusIcon, GearIcon, ShieldCheckIcon } from "../Icons";
import { isMac } from "../../lib/keybindings";

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

  // Agent spawn modal state
  const [agentSpawnModal, setAgentSpawnModal] = useState<{ folder: string; agentId: string } | null>(null);
  const [claudeSkipPerms, setClaudeSkipPerms] = useState(() => {
    try { return localStorage.getItem("codeterm-claude-skip-perms") === "true"; } catch { return false; }
  });
  const [claudeRememberChoice, setClaudeRememberChoice] = useState(() => {
    try { return localStorage.getItem("codeterm-claude-remember") === "true"; } catch { return false; }
  });

  const handleAgentSpawn = useCallback(async (agentId: string) => {
    const provider = getAgentProvider(agentId);
    let folder: string | null;
    try {
      folder = await open({ directory: true, multiple: false, title: `Choose project folder for ${provider?.displayName ?? agentId}` });
    } catch (err) {
      console.error(`[agent-spawn] folder picker failed:`, err);
      return;
    }
    if (!folder) return;
    // If user chose "remember", skip the modal and spawn directly
    if (claudeRememberChoice) {
      const wsId = state.active_workspace_id ?? state.workspaces[0]?.id;
      if (wsId) spawnAgentTab(wsId, agentId, claudeSkipPerms, folder);
      return;
    }
    setAgentSpawnModal({ folder, agentId });
  }, [state.active_workspace_id, state.workspaces, spawnAgentTab, claudeRememberChoice, claudeSkipPerms]);

  const handleClaudeConfirm = useCallback(() => {
    if (!agentSpawnModal) return;
    if (claudeRememberChoice) {
      try {
        localStorage.setItem("codeterm-claude-skip-perms", String(claudeSkipPerms));
        localStorage.setItem("codeterm-claude-remember", "true");
      } catch { /* ignore */ }
    }
    const wsId = state.active_workspace_id ?? state.workspaces[0]?.id;
    if (wsId) spawnAgentTab(wsId, agentSpawnModal.agentId, claudeSkipPerms, agentSpawnModal.folder);
    setAgentSpawnModal(null);
  }, [agentSpawnModal, claudeSkipPerms, claudeRememberChoice, state.active_workspace_id, state.workspaces, spawnAgentTab]);

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

  // Wrap closeTab to animate
  const animatedCloseTab = useCallback((wsId: string, tabId: string) => {
    setClosingTabs((prev) => {
      const next = new Map(prev);
      next.set(tabId, { wsId });
      return next;
    });
    // Phase 1: visual exit (150ms), then Phase 2: height collapse (200ms)
    setTimeout(() => {
      closeTab(wsId, tabId);
      setClosingTabs((prev) => {
        const next = new Map(prev);
        next.delete(tabId);
        return next;
      });
    }, 350);
  }, [closeTab]);

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
                              <span className="text-[10px]" style={{ color: tabProvider.iconColor }}>{tabProvider.icon}</span>
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
                  onNewAgent={handleAgentSpawn}
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

// ---------------------------------------------------------------------------
// Role icons & helpers
// ---------------------------------------------------------------------------

const ROLE_ICONS: Record<string, string> = {
  Orchestrator: "\u2605", // ★
  Conductor: "\u2605",    // ★ (backward compat)
  Lead: "\u00B7",         // ·
  Worker: "\u2699",       // ⚙
  SubWorker: "\u25E6",    // ◦
};

function RoleIcon({ role }: { role: AgentRole }) {
  return (
    <span className="shrink-0 text-[10px] leading-none" title={role}>
      {ROLE_ICONS[role] || "·"}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Hierarchy tree (rendered inside workspace when roles exist)
// ---------------------------------------------------------------------------

interface HierarchyNodeData {
  pane: Pane;
  children: HierarchyNodeData[];
  status: PaneStatus | null;
}

export function buildHierarchyTree(
  panes: Record<string, Pane>,
  statuses: Record<string, PaneStatus>,
): HierarchyNodeData[] {
  // Find orchestrators (top-level nodes)
  const orchestrators = Object.values(panes).filter(
    (p) => p.role === "Orchestrator" || p.role === "Conductor",
  );
  function buildNode(pane: Pane): HierarchyNodeData {
    const children = Object.values(panes)
      .filter((p) => p.parent_pane_id === pane.id)
      .map(buildNode);
    return { pane, children, status: statuses[pane.id] ?? null };
  }
  return orchestrators.map(buildNode);
}

export function HierarchyTree({
  nodes,
  depth,
  onJumpToPane,
  reportCounts,
}: {
  nodes: HierarchyNodeData[];
  depth: number;
  onJumpToPane: (paneId: string) => void;
  reportCounts: Record<string, number>;
}) {
  if (nodes.length === 0) return null;
  return (
    <div className={depth > 0 ? "ml-3 border-l border-[var(--ct-border)]" : ""}>
      {nodes.map((node) => {
        const rc = reportCounts[node.pane.id] || 0;
        return (
          <div key={node.pane.id}>
            <button
              onClick={() => onJumpToPane(node.pane.id)}
              className="flex items-center gap-1.5 w-full pl-2 pr-2 py-0.5 text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] transition-colors text-xs truncate"
              title={`${node.pane.title} (${node.pane.role})`}
            >
              {node.pane.role && <RoleIcon role={node.pane.role} />}
              <span className={`truncate ${node.pane.role === "Orchestrator" || node.pane.role === "Conductor" ? "font-semibold" : ""}`}>
                {node.pane.title}
              </span>
              <StatusDot status={node.status} animate />
              {node.pane.connected_services?.includes("telegram") && (
                <span className="text-[9px] text-[var(--ct-muted-fg)]/50">
                  <TelegramIcon />
                </span>
              )}
              {rc > 0 && (
                <span className="inline-flex items-center justify-center min-w-[14px] h-3.5 px-0.5 rounded-full bg-[var(--ct-accent)]/20 text-[var(--ct-accent)] text-[9px] font-bold leading-none">
                  {rc}
                </span>
              )}
            </button>
            <HierarchyTree
              nodes={node.children}
              depth={depth + 1}
              onJumpToPane={onJumpToPane}
              reportCounts={reportCounts}
            />
          </div>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Status dot component
// ---------------------------------------------------------------------------

const STATUS_COLORS: Record<PaneStatus, string> = {
  Working: "var(--ct-green, #7ee787)",
  Waiting: "var(--ct-yellow, #d29922)",
  Idle: "var(--ct-muted, #484f58)",
  Dead: "var(--ct-red, #f85149)",
};

function StatusDot({ status, animate }: { status: PaneStatus | null; animate?: boolean }) {
  if (!status || status === "Idle") return null;
  const color = STATUS_COLORS[status];
  return (
    <span
      className={`inline-block w-2 h-2 rounded-full shrink-0 ${animate && status === "Working" ? "animate-pulse" : ""}`}
      style={{ backgroundColor: color }}
      title={status}
    />
  );
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

interface WorkspaceItemProps {
  workspace: Workspace;
  isActive: boolean;
  isExpanded: boolean;
  onClick: () => void;
  onToggleExpand: () => void;
  onClose: () => void;
  onRename: (name: string) => void;
  onNewTerminal: () => void;
  onNewAgent: (agentId: string) => void;
  canClose: boolean;
  status: PaneStatus | null;
  hasWaiting: boolean;
  onDragOver?: (e: React.DragEvent) => void;
  onDrop?: (e: React.DragEvent) => void;
}

function WorkspaceItem({
  workspace,
  isActive,
  isExpanded,
  onClick,
  onToggleExpand,
  onClose,
  onRename,
  onNewTerminal,
  onNewAgent,
  canClose: _canClose,
  status: _status,
  hasWaiting,
  onDragOver,
  onDrop,
}: WorkspaceItemProps) {
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(workspace.name);
  const [dragOver, setDragOver] = useState(false);

  const handleDoubleClick = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    setEditName(workspace.name);
    setEditing(true);
  }, [workspace.name]);

  const commitRename = useCallback(() => {
    const trimmed = editName.trim();
    if (trimmed && trimmed !== workspace.name) {
      onRename(trimmed);
    }
    setEditing(false);
  }, [editName, workspace.name, onRename]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Enter") { e.preventDefault(); commitRename(); }
    if (e.key === "Escape") { setEditing(false); }
  }, [commitRename]);

  const menuItems: DropdownMenuItem[] = useMemo(() => {
    const items: DropdownMenuItem[] = [
      {
        label: "New Terminal",
        icon: <TerminalSquare size={11} strokeWidth={1.5} />,
        onClick: onNewTerminal,
      },
    ];
    // Dynamic agent provider entries from registry
    for (const provider of getAllProviders()) {
      items.push({
        label: `New ${provider.displayName} Session`,
        icon: <span className="text-[11px] leading-none" style={{ color: provider.iconColor }}>{provider.icon}</span>,
        onClick: () => onNewAgent(provider.id),
      });
    }
    if (workspace.cwd) {
      items.push({
        label: isMac ? "Show in Finder" : "Show in Explorer",
        icon: <FolderOpen size={11} strokeWidth={1.5} />,
        onClick: () => {
          invoke("open_folder", { path: workspace.cwd }).catch(() => {});
        },
      });
    }
    if (!workspace.protected) {
      items.push({
        label: "Delete Workspace",
        icon: <X size={11} strokeWidth={2} />,
        onClick: onClose,
        danger: true,
      });
    }
    return items;
  }, [onNewTerminal, onNewAgent, onClose, workspace.protected, workspace.cwd]);

  return (
    <div
      className={`
        group flex items-center justify-between px-1 py-1.5 cursor-pointer transition-colors
        ${isActive ? "bg-[var(--ct-active)] text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"}
        ${hasWaiting ? "bg-[var(--ct-yellow)]/10" : ""}
        ${dragOver ? "bg-[var(--ct-accent)]/20" : ""}
      `}
      onDragOver={(e) => {
        e.preventDefault();
        setDragOver(true);
        onDragOver?.(e);
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={(e) => {
        setDragOver(false);
        onDrop?.(e);
      }}
    >
      <div className="flex items-center gap-1 min-w-0">
        {/* Chevron toggle — rotates on expand/collapse */}
        <button
          onClick={(e) => {
            e.stopPropagation();
            onToggleExpand();
          }}
          className="flex items-center justify-center w-4 h-4 shrink-0 text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] transition-colors"
          title={isExpanded ? "Collapse" : "Expand"}
        >
          <svg
            width="8"
            height="8"
            viewBox="0 0 8 8"
            fill="currentColor"
            className="transition-transform duration-[320ms]"
            style={{
              transform: isExpanded ? "rotate(90deg)" : "rotate(0deg)",
              transitionTimingFunction: "cubic-bezier(0.16, 1, 0.3, 1)",
            }}
          >
            <path d="M2 1l4 3-4 3V1z" />
          </svg>
        </button>
        {/* Icon + name (click to activate, double-click to rename) */}
        <div className="flex items-center gap-2 min-w-0" onClick={onClick}>
          {workspace.protected ? (
            <span className="relative shrink-0 flex items-center justify-center w-[14px] h-[14px]">
              <ShieldCheckIcon style={{ stroke: "var(--ct-muted-fg)" }} className="shrink-0" />
            </span>
          ) : (
          <FolderIcon open={isExpanded} className="shrink-0" />
          )}
          {editing ? (
            <input
              autoFocus
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
              onBlur={commitRename}
              onKeyDown={handleKeyDown}
              onClick={(e) => e.stopPropagation()}
              className="text-xs bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded px-1 py-0 w-full outline-none"
            />
          ) : (
            <span className="text-xs font-medium truncate" onDoubleClick={handleDoubleClick} title={workspace.cwd || undefined}>
              {workspace.name}
            </span>
          )}
        </div>
        <span className="text-[10px] text-[var(--ct-muted)]">
          {workspace.tabs.length}
        </span>
        {hasWaiting && (
          <span className="inline-flex items-center justify-center min-w-[14px] h-3.5 px-0.5 rounded-full bg-[var(--ct-yellow)]/20 text-[var(--ct-yellow)] text-[9px] font-bold leading-none">
            !
          </span>
        )}
      </div>
      {/* 3-dot menu — visible on hover */}
      <DropdownMenu
        items={menuItems}
        className="shrink-0 opacity-0 group-hover:opacity-60 hover:!opacity-100 transition-opacity"
        trigger={
          <button
            className="w-5 h-5 flex items-center justify-center rounded hover:bg-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors"
            title="Workspace actions"
          >
            <span className="text-[13px] leading-none font-bold">{"\u22EE"}</span>
          </button>
        }
      />
    </div>
  );
}

interface TabItemProps {
  tab: Tab;
  index: number;
  isActive: boolean;
  onClick: () => void;
  onClose: () => void;
  canClose: boolean;
  paneCount: number;
  workspaceId: string;
  hasAgent?: boolean;
  status?: PaneStatus | null;
  onRename?: (title: string) => void;
  pane?: Pane | null;
  orchestrators?: Pane[];
  isAppearing?: boolean;
  isClosing?: boolean;
}

function TabItem({
  tab,
  isActive,
  onClick,
  onClose,
  canClose,
  paneCount,
  workspaceId,
  hasAgent,
  status,
  onRename: _onRename,
  pane,
  orchestrators: _orchestrators,
  isAppearing,
  isClosing,
}: TabItemProps) {
  const agentProvider = getAgentProvider(pane?.agent_type);
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(tab.title);
  const tabRef = useRef<HTMLDivElement>(null);

  // Close animation phases
  const [closePhase, setClosePhase] = useState<0 | 1 | 2>(0);
  useEffect(() => {
    if (isClosing && closePhase === 0) {
      setClosePhase(1); // Phase 1: visual exit
      const timer = setTimeout(() => setClosePhase(2), 150); // Phase 2: height collapse
      return () => clearTimeout(timer);
    }
  }, [isClosing, closePhase]);

  // Build menu items for the 3-dot dropdown
  const menuItems: DropdownMenuItem[] = useMemo(() => {
    const items: DropdownMenuItem[] = [];
    items.push({
      label: "Rename",
      icon: <Pencil size={11} strokeWidth={2} />,
      onClick: () => { setEditName(tab.title); setEditing(true); },
    });
    // Role management — hide for Orchestrators (default role on all tabs)
    if (pane?.role && pane.role !== "Orchestrator" && pane.role !== "Conductor") {
      items.push({
        label: "Remove Role",
        icon: <UserMinus size={11} strokeWidth={2} />,
        onClick: () => { if (pane) invoke("mux_clear_pane_role", { paneId: pane.id }); },
      });
    }
    if (pane?.role && pane.role !== "Orchestrator" && (pane.role === "Conductor" || pane.role === "Lead")) {
      items.push({
        label: pane.can_spawn ? "Disallow Spawning" : "Allow Spawning",
        icon: pane.can_spawn
          ? <Ban size={11} strokeWidth={2} />
          : <CircleCheck size={11} strokeWidth={2} />,
        onClick: () => { if (pane) invoke("mux_set_can_spawn", { paneId: pane.id, canSpawn: !pane.can_spawn }); },
      });
    }
    if (canClose && !pane?.protected) {
      items.push({
        label: hasAgent ? "Close Session" : "Close Tab",
        icon: <X size={11} strokeWidth={2} />,
        onClick: onClose,
        danger: true,
      });
    }
    return items;
  }, [tab.title, pane, canClose, hasAgent, onClose]);

  // Build animation CSS class
  const animClass = isClosing
    ? closePhase === 2
      ? "sidebar-tab-closing-2"
      : "sidebar-tab-closing-1"
    : isAppearing
      ? "sidebar-tab-appear"
      : "";

  return (
    <div
      ref={tabRef}
      onClick={onClick}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData("text/plain", JSON.stringify({
          tabId: tab.id,
          fromWorkspaceId: workspaceId,
        }));
        e.dataTransfer.effectAllowed = "move";
      }}
      className={`
        sidebar-tab group flex items-center justify-between pl-8 pr-2 py-1 cursor-pointer transition-colors relative
        ${animClass}
        ${isActive ? "text-[var(--ct-fg)]" : "sidebar-tab-dimmed hover:bg-[rgba(255,255,255,0.04)]"}
      `}
    >
      {/* Active tab sliding pill indicator */}
      {isActive && (
        <div
          className="absolute inset-y-0 left-0 right-0 pointer-events-none"
          style={{
            background: "var(--ct-bg-selected, rgba(99, 102, 241, 0.12))",
            transition: "transform 320ms cubic-bezier(0.16, 1, 0.3, 1), opacity 200ms ease",
          }}
        />
      )}
      <div className="flex items-center gap-2 min-w-0 relative z-[1]">
        {/* Icon — Lead/Worker role icon > orange sparkle for Claude Code (spins when working) > terminal icon */}
        {/* Orchestrator uses standard icon (Claude sparkle or terminal) — it's just a user tab */}
        {pane?.role && pane.role !== "Orchestrator" && pane.role !== "Conductor" ? (
          <span
            className={`shrink-0 text-[11px] leading-none text-[var(--ct-muted-fg)] ${status === "Working" ? "animate-pulse" : ""}`}
            title={`${pane.role}${status === "Working" ? " — working" : ""}`}
          >{ROLE_ICONS[pane.role]}</span>
        ) : hasAgent ? (
          <span
            className={`shrink-0 text-[11px] leading-none ${status === "Working" ? "animate-spin" : ""}`}
            style={{ color: agentProvider?.iconColor || "#f0883e", ...(status === "Working" ? { animationDuration: "1.5s" } : {}) }}
            title={status === "Working" ? `${agentProvider?.displayName || "Agent"} working...` : status === "Waiting" ? `${agentProvider?.displayName || "Agent"} — waiting for input` : agentProvider?.displayName || "Agent"}
          >{agentProvider?.icon || "\u2733"}</span>
        ) : (
          <TerminalIcon className="shrink-0" />
        )}
        {editing ? (
          <input
            autoFocus
            value={editName}
            onChange={(e) => setEditName(e.target.value)}
            onClick={(e) => e.stopPropagation()}
            onBlur={() => {
              const trimmed = editName.trim();
              setEditing(false);
              if (trimmed && trimmed !== tab.title && _onRename) _onRename(trimmed);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                (e.target as HTMLInputElement).blur();
              } else if (e.key === "Escape") {
                setEditName(tab.title);
                setEditing(false);
              }
            }}
            className="text-xs bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded px-1 py-0 w-full outline-none"
          />
        ) : (
          <span
            className={`text-xs truncate ${isActive ? "font-medium" : ""}`}
            onDoubleClick={(e) => { e.stopPropagation(); setEditName(tab.title); setEditing(true); }}
          >
            {hasAgent ? tab.title.replace(/^[✻✳✴✵✶✷✸✹✺✻✼✽✾✿❀❁❂❃❇❈❉❊❋*]\s*/u, "") : tab.title}
          </span>
        )}
        {/* Connected services */}
        {pane?.connected_services?.includes("telegram") && (
          <span className="shrink-0 text-[var(--ct-muted-fg)]/50">
            <TelegramIcon />
          </span>
        )}
        {/* Prompt/status indicator dot */}
        {pane?.pending_prompt && (
          <span
            className="shrink-0 w-2 h-2 rounded-full animate-pulse"
            style={{ background: "var(--ct-yellow)" }}
            title={pane.pending_prompt.question}
          />
        )}
        {paneCount > 1 && (
          <span className="flex items-center gap-0.5 text-[var(--ct-muted)]" title={`${paneCount} panes`}>
            <svg width="12" height="10" viewBox="0 0 14 10" fill="none" stroke="currentColor" strokeWidth="1">
              {paneCount === 2 ? (
                /* Two panes side by side */
                <><rect x="0.5" y="0.5" width="13" height="9" rx="1" /><line x1="7" y1="0.5" x2="7" y2="9.5" /></>
              ) : paneCount === 3 ? (
                /* Three panes: one left, two right stacked */
                <><rect x="0.5" y="0.5" width="13" height="9" rx="1" /><line x1="7" y1="0.5" x2="7" y2="9.5" /><line x1="7" y1="5" x2="13.5" y2="5" /></>
              ) : (
                /* Four+ panes: grid */
                <><rect x="0.5" y="0.5" width="13" height="9" rx="1" /><line x1="7" y1="0.5" x2="7" y2="9.5" /><line x1="0.5" y1="5" x2="13.5" y2="5" /></>
              )}
            </svg>
          </span>
        )}
      </div>
      {/* 3-dot menu — visible on hover */}
      {canClose && (
        <DropdownMenu
          items={menuItems}
          className="sidebar-tab-close shrink-0 relative z-[1]"
          trigger={
            <button
              className="w-5 h-5 flex items-center justify-center rounded text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-border)] transition-colors"
              title="Tab actions"
            >
              <span className="text-[13px] leading-none font-bold">{"\u22EE"}</span>
            </button>
          }
        />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Cron Jobs section (rendered per workspace, read-only — create via CLI/API)
// ---------------------------------------------------------------------------



function CronSection({
  workspaceId,
  onOpenCron,
  activeCronId,
}: {
  workspaceId: string;
  onOpenCron?: (jobId?: string) => void;
  activeCronId?: string;
}) {
  const { crons, update, remove } = useCrons();
  const [expanded, setExpanded] = useState(true);
  const [editingCron, setEditingCron] = useState<CronJob | null>(null);

  const wsCrons = crons.filter((c) => c.workspace_id === workspaceId);

  if (wsCrons.length === 0) return null;

  return (
    <>
      <div className="flex items-center mx-2 mt-2 mb-0.5 px-3">
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-1.5 text-[10px] uppercase tracking-[0.06em] font-medium text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)]"
        >
          <span className="text-[9px]">{expanded ? "\u25BC" : "\u25B6"}</span>
          Periodic Tasks ({wsCrons.length})
        </button>
      </div>

      {expanded &&
        wsCrons.map((job) => (
          <CronItem
            key={job.id}
            job={job}
            onOpen={() => onOpenCron?.(job.id)}
            isActive={activeCronId === job.id}
          />
        ))}

      {editingCron && (
        <CronEditor
          workspaceId={workspaceId}
          existing={editingCron}
          onClose={() => setEditingCron(null)}
          onSave={async (data) => {
            await update(editingCron.id, {
              ...data,
              cwd: data.cwd ?? undefined,
              description: data.description ?? undefined,
            });
            setEditingCron(null);
          }}
          onDelete={async () => {
            await remove(editingCron.id);
            setEditingCron(null);
          }}
        />
      )}
    </>
  );
}

function CronItem({
  job,
  onOpen,
  isActive,
}: {
  job: CronJob;
  onOpen: () => void;
  isActive: boolean;
}) {
  // Status dot reflects the most recent run:
  //   running → pulsing accent
  //   enabled + idle → green
  //   disabled → muted
  const running = job.last_run?.status === "Running";
  const dotColor = running
    ? "bg-[var(--ct-accent)] animate-pulse"
    : job.enabled
      ? "bg-[var(--ct-green)]"
      : "bg-[var(--ct-muted)]";

  return (
    <div
      className={`group flex items-center gap-1.5 mx-2 px-3 py-1 rounded-md transition-colors cursor-pointer ${
        isActive
          ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
          : job.enabled
          ? "text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
          : "text-[var(--ct-muted-fg)] opacity-80 hover:bg-[var(--ct-hover)]"
      }`}
      onClick={onOpen}
      style={{ paddingLeft: 20 }}
      title={running ? "Running — click to view live log" : "Click to open task settings"}
    >
      <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${dotColor}`} />
      <span className="text-[11px] truncate flex-1">{job.name}</span>
      {job.enabled && job.next_run && !running && (
        <span className="text-[9px] text-[var(--ct-muted)] shrink-0">
          {new Date(job.next_run).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </span>
      )}
    </div>
  );
}

