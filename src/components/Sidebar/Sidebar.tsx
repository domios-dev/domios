import { useCallback, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useMultiplexer } from "../../store/multiplexer";
import { TitleLevel } from "../../lib/types";
import type { Workspace, Tab, SplitNode, PaneStatus, Pane, AgentRole } from "../../lib/types";

function collectPaneIds(node: SplitNode): string[] {
  if (node.type === "Leaf") return [node.pane_id];
  return node.children.flatMap(collectPaneIds);
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
}

export function Sidebar({ onOpenSettings }: SidebarProps) {
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
  } = useMultiplexer();

  // Claude spawn modal state
  const [claudeSpawnModal, setClaudeSpawnModal] = useState<{ folder: string } | null>(null);
  const [claudeSkipPerms, setClaudeSkipPerms] = useState(() => {
    try { return localStorage.getItem("codeterm-claude-skip-perms") === "true"; } catch { return false; }
  });
  const [claudeRememberChoice, setClaudeRememberChoice] = useState(() => {
    try { return localStorage.getItem("codeterm-claude-remember") === "true"; } catch { return false; }
  });

  const handleClaudeSpawn = useCallback(async () => {
    let folder: string | null;
    try {
      folder = await open({ directory: true, multiple: false, title: "Choose project folder for Claude" });
    } catch (err) {
      console.error("[claude-spawn] folder picker failed:", err);
      return;
    }
    if (!folder) return;
    const path = folder;
    console.log("[claude-spawn] folder picked:", path);
    // If user chose "remember", skip the modal and spawn directly
    if (claudeRememberChoice) {
      const wsId = state.active_workspace_id ?? state.workspaces[0]?.id;
      console.log("[claude-spawn] remember mode, wsId:", wsId, "skip:", claudeSkipPerms);
      if (wsId) spawnClaudeTab(wsId, claudeSkipPerms, path);
      return;
    }
    setClaudeSpawnModal({ folder: path });
  }, [state.active_workspace_id, state.workspaces, spawnClaudeTab, claudeRememberChoice, claudeSkipPerms]);

  const handleClaudeConfirm = useCallback(() => {
    if (!claudeSpawnModal) return;
    if (claudeRememberChoice) {
      try {
        localStorage.setItem("codeterm-claude-skip-perms", String(claudeSkipPerms));
        localStorage.setItem("codeterm-claude-remember", "true");
      } catch { /* ignore */ }
    }
    const wsId = state.active_workspace_id ?? state.workspaces[0]?.id;
    if (wsId) spawnClaudeTab(wsId, claudeSkipPerms, claudeSpawnModal.folder);
    setClaudeSpawnModal(null);
  }, [claudeSpawnModal, claudeSkipPerms, claudeRememberChoice, state.active_workspace_id, state.workspaces, spawnClaudeTab]);

  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Set<string>>(
    () => new Set(state.active_workspace_id ? [state.active_workspace_id] : []),
  );

  // Auto-expand only the active workspace when it changes
  useMemo(() => {
    if (state.active_workspace_id) {
      setExpandedWorkspaces((prev) => {
        const next = new Set(prev);
        next.add(state.active_workspace_id!);
        return next;
      });
    }
  }, [state.active_workspace_id]);

  const handleNewWorkspace = useCallback(() => {
    const n = state.workspaces.length + 1;
    createWorkspace(`Workspace ${n}`);
  }, [state.workspaces.length, createWorkspace]);

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

  return (
    <>
      <aside
        ref={sidebarRef}
        className="shrink-0 bg-[var(--ct-sidebar-bg)] border-r border-[var(--ct-border)] flex flex-col select-none overflow-hidden relative"
        style={{ width: sidebarWidth }}
      >
        {/* Panes header with action icons */}
        <div className={`flex items-center ${isCompact ? "justify-center" : "justify-between"} px-3 py-2`}>
          {!isCompact && (
            <span className="text-[10px] uppercase tracking-[0.06em] font-medium text-[var(--ct-muted)] truncate">
              Panes
            </span>
          )}
          <div className="flex items-center gap-1">
            {/* Spawn Claude */}
            <button
              onClick={handleClaudeSpawn}
              className="flex items-center justify-center w-6 h-6 rounded border border-[#d4a574]/30 bg-[#d4a574]/5 hover:bg-[#d4a574]/15 hover:border-[#d4a574]/50 text-[#d4a574]/70 hover:text-[#d4a574] transition-colors"
              title="Spawn Claude Code"
            >
              <span className="text-[15px] leading-none">✳</span>
            </button>
            {/* New terminal pane */}
            <button
              onClick={() => {
                const ws = state.workspaces.find((w) => w.id === state.active_workspace_id);
                if (ws) createTab(ws.id);
              }}
              className="flex items-center justify-center w-6 h-6 rounded border border-[var(--ct-border)] hover:bg-[var(--ct-hover)] hover:border-[var(--ct-muted)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
              title="New terminal"
            >
              <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
                <path d="M2 3l4 4-4 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                <path d="M8 13h6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
              </svg>
            </button>
            {/* New workspace (group) */}
            <button
              onClick={handleNewWorkspace}
              className="flex items-center justify-center w-6 h-6 rounded border border-[var(--ct-border)] hover:bg-[var(--ct-hover)] hover:border-[var(--ct-muted)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
              title="New workspace"
            >
              <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
                <path d="M1.5 3.5A1 1 0 012.5 2.5h3l1.5 2h6.5a1 1 0 011 1v7a1 1 0 01-1 1h-11a1 1 0 01-1-1v-9z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
                <path d="M8 7v4M6 9h4" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
              </svg>
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
                const isExpanded = expandedWorkspaces.has(ws.id);
                return (
                  <div key={ws.id} className="w-full flex flex-col items-center">
                    <button
                      onClick={() => { setActiveWorkspace(ws.id); toggleExpanded(ws.id); }}
                      className={`w-full py-1.5 flex items-center justify-center transition-colors ${
                        isActive ? "text-[var(--ct-fg)]" : "text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)]"
                      }`}
                      title={ws.name}
                    >
                      <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor" className={isExpanded ? "opacity-70" : "opacity-40"}>
                        {isExpanded ? (
                          <path d="M.513 1.513A1.75 1.75 0 0 1 1.75 1h3.5c.55 0 1.07.26 1.4.7l.9 1.2a.25.25 0 0 0 .2.1h6.5A1.75 1.75 0 0 1 16 4.75v.462a.25.25 0 0 1-.03.118l-2.39 4.608a1.75 1.75 0 0 1-1.553.937H1.75A1.75 1.75 0 0 1 0 9.125V2.75c0-.464.184-.91.513-1.237Z" />
                        ) : (
                          <path d="M1.75 1A1.75 1.75 0 0 0 0 2.75v10.5C0 14.216.784 15 1.75 15h12.5A1.75 1.75 0 0 0 16 13.25v-8.5A1.75 1.75 0 0 0 14.25 3H7.5a.25.25 0 0 1-.2-.1l-.9-1.2C6.07 1.26 5.55 1 5 1H1.75Z" />
                        )}
                      </svg>
                    </button>
                    {isExpanded && ws.tabs.map((tab) => {
                      const tabPaneIds = collectPaneIds(tab.split_tree);
                      const tabHasAgent = tabPaneIds.some((pid) => state.panes[pid]?.agent_type === "claude");
                      const isTabActive = isActive && tab.id === ws.active_tab_id;
                      return (
                        <button
                          key={tab.id}
                          onClick={() => { setActiveWorkspace(ws.id); setActiveTab(ws.id, tab.id); }}
                          className={`w-8 h-7 flex items-center justify-center rounded transition-colors ${
                            isTabActive
                              ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                              : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-muted-fg)]"
                          }`}
                          title={tab.title}
                        >
                          {tabHasAgent ? (
                            <span className="text-[10px] text-[#f0883e]">&#10035;</span>
                          ) : (
                            <svg width="11" height="11" viewBox="0 0 16 16" fill="currentColor">
                              <path d="M0 2.75C0 1.784.784 1 1.75 1h12.5c.966 0 1.75.784 1.75 1.75v10.5A1.75 1.75 0 0 1 14.25 15H1.75A1.75 1.75 0 0 1 0 13.25Zm1.75-.25a.25.25 0 0 0-.25.25v10.5c0 .138.112.25.25.25h12.5a.25.25 0 0 0 .25-.25V2.75a.25.25 0 0 0-.25-.25ZM3.5 4.5l3 2.5-3 2.5V4.5ZM8.5 10h3v1.5h-3V10Z" />
                            </svg>
                          )}
                        </button>
                      );
                    })}
                    <div className="w-5 h-px bg-[var(--ct-border)] my-0.5" />
                  </div>
                );
              })}
            </div>
          ) : state.workspaces.map((ws) => {
            const allPaneIds = ws.tabs.flatMap((t) => collectPaneIds(t.split_tree));
            const wsStatus = aggregateStatus(allPaneIds, state.paneStatuses);
            const hasWaiting = allPaneIds.some(
              (pid) => state.paneStatuses[pid] === "Waiting",
            );
            const isActive = ws.id === state.active_workspace_id;
            const isExpanded = expandedWorkspaces.has(ws.id);
            // Orchestrators available for linking (across all workspaces)
            const wsOrchestrators = Object.values(state.panes).filter(
              (p) => p.role === "Orchestrator" || p.role === "Conductor" || p.role === "Lead" || (p.role === "Worker" && p.can_spawn),
            );

            return (
              <div key={ws.id}>
                <WorkspaceItem
                  workspace={ws}
                  isActive={isActive}
                  isExpanded={isExpanded}
                  onClick={() => setActiveWorkspace(ws.id)}
                  onToggleExpand={() => toggleExpanded(ws.id)}
                  onClose={() => closeWorkspace(ws.id)}
                  onRename={(name) => renameWorkspace(ws.id, name)}
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
                {isExpanded ? (
                  <>
                    {(() => {
                      // Build pane→tab lookup and find which tabs are children
                      const paneToTab: Record<string, typeof ws.tabs[0]> = {};
                      for (const tab of ws.tabs) {
                        const pids = collectPaneIds(tab.split_tree);
                        if (pids[0]) paneToTab[pids[0]] = tab;
                      }
                      // Set of pane IDs that are children (have a parent) — skip at top level
                      const childPaneIds = new Set<string>();
                      for (const tab of ws.tabs) {
                        const pids = collectPaneIds(tab.split_tree);
                        const pane = pids[0] ? state.panes[pids[0]] : null;
                        if (pane?.parent_pane_id) childPaneIds.add(pids[0]);
                      }

                      // Render a tab + its children recursively
                      const renderTab = (tab: Tab, index: number, depth: number): React.ReactNode => {
                        const tabPids = collectPaneIds(tab.split_tree);
                        const firstPane = tabPids[0] ? state.panes[tabPids[0]] : null;
                        // Find child tabs (tabs whose pane.parent_pane_id === this pane)
                        const childTabs = firstPane
                          ? ws.tabs.filter((t) => {
                              const tPids = collectPaneIds(t.split_tree);
                              const tPane = tPids[0] ? state.panes[tPids[0]] : null;
                              return tPane?.parent_pane_id === firstPane.id;
                            })
                          : [];

                        return (
                          <div key={tab.id} style={depth > 0 ? { paddingLeft: depth * 12 } : undefined}>
                            <TabItem
                              tab={tab}
                              index={index}
                              isActive={isActive && tab.id === ws.active_tab_id}
                              onClick={() => {
                                setActiveWorkspace(ws.id);
                                setActiveTab(ws.id, tab.id);
                              }}
                              onClose={() => closeTab(ws.id, tab.id)}
                              canClose={true}
                              paneCount={tabPids.length}
                              workspaceId={ws.id}
                              hasAgent={tabPids.some(
                                (pid) => state.panes[pid]?.agent_type === "claude"
                              )}
                              status={aggregateStatus(tabPids, state.paneStatuses)}
                              onRename={(title) => {
                                if (tabPids[0]) {
                                  setPaneTitle(tabPids[0], title, TitleLevel.User);
                                  invoke("mux_rename_pane", { paneId: tabPids[0], title }).then(() => {
                                  }).catch((e) => {
                                  });
                                }
                              }}
                              pane={firstPane}
                              orchestrators={wsOrchestrators}
                            />
                            {childTabs.map((ct, ci) => renderTab(ct, ci, depth + 1))}
                          </div>
                        );
                      };

                      // Only render top-level tabs (not children of other tabs)
                      return ws.tabs
                        .filter((tab) => {
                          const pids = collectPaneIds(tab.split_tree);
                          return !childPaneIds.has(pids[0]);
                        })
                        .map((tab, index) => renderTab(tab, index, 0));
                    })()}
                    {/* Add tab button */}
                    <button
                      onClick={() => createTab(ws.id)}
                      className="flex items-center gap-2 mx-2 mt-1 mb-0.5 px-5 py-1 rounded-md text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] transition-colors"
                      title="New tab in this workspace"
                    >
                      <svg width="10" height="10" viewBox="0 0 12 12" fill="none">
                        <path d="M6 1v10M1 6h10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
                      </svg>
                      <span className="text-[11px]">New tab</span>
                    </button>
                  </>
                ) : isActive && ws.active_tab_id ? (
                  /* Collapsed but active — show selected tab */
                  (() => {
                    const activeTab = ws.tabs.find((t) => t.id === ws.active_tab_id);
                    if (!activeTab) return null;
                    const atPids = collectPaneIds(activeTab.split_tree);
                    const atFirstPane = atPids[0] ? state.panes[atPids[0]] : null;
                    return (
                      <TabItem
                        tab={activeTab}
                        index={0}
                        isActive={true}
                        onClick={() => setActiveTab(ws.id, activeTab.id)}
                        onClose={() => closeTab(ws.id, activeTab.id)}
                        canClose={true}
                        paneCount={atPids.length}
                        workspaceId={ws.id}
                        hasAgent={atPids.some(
                          (pid) => state.panes[pid]?.agent_type === "claude"
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

        {/* Settings button at bottom */}
        <div className={`border-t border-[var(--ct-border)] ${isCompact ? "px-1 py-2.5 flex justify-center" : "px-3 py-2.5"}`}>
          <button
            onClick={onOpenSettings}
            className={`flex items-center ${isCompact ? "justify-center w-8 h-8" : "gap-2 w-full px-2 py-1.5"} rounded-md text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] transition-colors`}
            title="Settings (Ctrl+,)"
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" className="shrink-0">
              <path d="M8 0a8.2 8.2 0 0 1 .701.031C9.444.095 9.99.645 10.16 1.29l.288 1.107c.018.066.079.158.212.224.231.114.454.243.668.386.123.082.233.09.3.071l1.102-.303c.644-.176 1.392.021 1.82.63.27.385.506.792.704 1.218.315.675.111 1.422-.364 1.891l-.814.806c-.049.048-.098.147-.088.294a6.57 6.57 0 0 1 0 .772c-.01.147.04.246.088.294l.814.806c.475.469.679 1.216.364 1.891a7.977 7.977 0 0 1-.704 1.217c-.428.61-1.176.807-1.82.63l-1.103-.303c-.066-.019-.176-.011-.299.071a5.909 5.909 0 0 1-.668.386c-.133.066-.194.158-.212.224l-.288 1.107c-.17.645-.716 1.195-1.459 1.26a8.006 8.006 0 0 1-1.402 0c-.743-.065-1.289-.615-1.459-1.26l-.289-1.107c-.017-.066-.078-.158-.211-.224a5.909 5.909 0 0 1-.668-.386c-.123-.082-.233-.09-.3-.071l-1.102.303c-.644.176-1.392-.021-1.82-.63a8.012 8.012 0 0 1-.704-1.218c-.315-.675-.111-1.422.363-1.891l.815-.806c.049-.048.098-.147.088-.294a6.57 6.57 0 0 1 0-.772c.01-.147-.04-.246-.088-.294l-.815-.806C.635 6.045.431 5.298.746 4.623a7.92 7.92 0 0 1 .704-1.217c.428-.61 1.176-.807 1.82-.63l1.103.303c.066.019.176.011.299-.071.214-.143.437-.272.668-.386.133-.066.194-.158.212-.224L5.84 1.29c.17-.645.716-1.195 1.459-1.26A8.2 8.2 0 0 1 8 0ZM5.5 8a2.5 2.5 0 1 0 5 0 2.5 2.5 0 0 0-5 0Z" />
            </svg>
            {!isCompact && <span className="text-[11px]">Settings</span>}
          </button>
        </div>
        {/* Resize handle */}
        <div
          onMouseDown={handleSidebarDragStart}
          className="absolute right-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-[var(--ct-accent)] transition-colors"
        />
      </aside>

      {/* Claude spawn modal */}
      {claudeSpawnModal && (
        <>
          <div className="fixed inset-0 z-50 bg-black/40" onClick={() => setClaudeSpawnModal(null)} />
          <div className="fixed z-50 top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-lg shadow-2xl w-[320px] overflow-hidden">
            <div className="px-4 pt-4 pb-2">
              <div className="text-[13px] font-medium text-[var(--ct-fg)]">Spawn Claude Code</div>
              <div className="mt-2 text-[11px] text-[var(--ct-muted)] truncate" title={claudeSpawnModal.folder}>
                {claudeSpawnModal.folder}
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
                onClick={() => setClaudeSpawnModal(null)}
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
  canClose: _canClose,
  status: _status,
  hasWaiting,
  onDragOver,
  onDrop,
}: WorkspaceItemProps) {
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(workspace.name);
  const [dragOver, setDragOver] = useState(false);

  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number } | null>(null);
  const [confirmClose, setConfirmClose] = useState(false);

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setCtxMenu({ x: e.clientX, y: e.clientY });
  }, []);

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

  return (
    <div
      className={`
        group flex items-center justify-between px-1 py-1.5 cursor-pointer transition-colors
        ${isActive ? "bg-[var(--ct-active)] text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"}
        ${hasWaiting ? "bg-[var(--ct-yellow)]/10" : ""}
        ${dragOver ? "bg-[var(--ct-accent)]/20" : ""}
      `}
      onContextMenu={handleContextMenu}
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
        {/* Chevron toggle */}
        <button
          onClick={(e) => {
            e.stopPropagation();
            onToggleExpand();
          }}
          className="flex items-center justify-center w-4 h-4 shrink-0 text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] transition-colors"
          title={isExpanded ? "Collapse" : "Expand"}
        >
          <span className="text-[10px] leading-none">{isExpanded ? "\u25BE" : "\u25B8"}</span>
        </button>
        {/* Folder icon + name (click to activate, double-click to rename) */}
        <div className="flex items-center gap-2 min-w-0" onClick={onClick}>
          <svg
            width="14"
            height="14"
            viewBox="0 0 16 16"
            fill="currentColor"
            className="shrink-0"
          >
            {isExpanded ? (
              <path d="M.513 1.513A1.75 1.75 0 0 1 1.75 1h3.5c.55 0 1.07.26 1.4.7l.9 1.2a.25.25 0 0 0 .2.1h6.5A1.75 1.75 0 0 1 16 4.75v.462a.25.25 0 0 1-.03.118l-2.39 4.608a1.75 1.75 0 0 1-1.553.937H1.75A1.75 1.75 0 0 1 0 9.125V2.75c0-.464.184-.91.513-1.237Z" />
            ) : (
              <path d="M1.75 1A1.75 1.75 0 0 0 0 2.75v10.5C0 14.216.784 15 1.75 15h12.5A1.75 1.75 0 0 0 16 13.25v-8.5A1.75 1.75 0 0 0 14.25 3H7.5a.25.25 0 0 1-.2-.1l-.9-1.2C6.07 1.26 5.55 1 5 1H1.75Z" />
            )}
          </svg>
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
            <span className="text-xs font-medium truncate" onDoubleClick={handleDoubleClick}>
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
      {/* Close button — visible on hover, always confirms */}
      <button
        onClick={(e) => { e.stopPropagation(); setConfirmClose(true); }}
        className="shrink-0 w-5 h-5 flex items-center justify-center rounded opacity-0 group-hover:opacity-60 hover:!opacity-100 hover:bg-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-all"
        title="Close workspace"
      >
        <svg width="7" height="7" viewBox="0 0 10 10" fill="currentColor">
          <path d="M1.41 0L5 3.59 8.59 0 10 1.41 6.41 5 10 8.59 8.59 10 5 6.41 1.41 10 0 8.59 3.59 5 0 1.41Z" />
        </svg>
      </button>
      {/* Context menu */}
      {ctxMenu && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setCtxMenu(null)} onContextMenu={(e) => { e.preventDefault(); setCtxMenu(null); }} />
          <div
            className="fixed z-50 w-36 bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-md shadow-lg py-1"
            style={{ left: ctxMenu.x, top: ctxMenu.y }}
          >
            <button
              onClick={() => { setCtxMenu(null); setEditName(workspace.name); setEditing(true); }}
              className="flex items-center gap-2 w-full px-3 py-1 text-xs text-[var(--ct-fg)] hover:bg-[var(--ct-border)]"
            >Rename</button>
            <button
              onClick={() => { setCtxMenu(null); setConfirmClose(true); }}
              className="flex items-center gap-2 w-full px-3 py-1 text-xs text-[var(--ct-red)] hover:bg-[var(--ct-border)]"
            >Close</button>
          </div>
        </>
      )}
      {confirmClose && (
        <ConfirmStopDialog
          message={`Close workspace "${workspace.name}"?`}
          onConfirm={() => { onClose(); setConfirmClose(false); }}
          onCancel={() => setConfirmClose(false)}
        />
      )}
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
}

function ConfirmStopDialog({ onConfirm, onCancel, message }: { onConfirm: () => void; onCancel: () => void; message?: string }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={onCancel}>
      <div
        className="bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-lg shadow-xl px-6 py-5 max-w-sm text-center"
        onClick={(e) => e.stopPropagation()}
      >
        <p className="text-sm text-[var(--ct-fg)] mb-4">{message || "Are you sure you want to stop the process?"}</p>
        <div className="flex items-center justify-center gap-3">
          <button
            onClick={onCancel}
            className="px-4 py-1.5 text-xs rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            className="px-4 py-1.5 text-xs rounded bg-[var(--ct-red)] hover:opacity-80 text-white font-medium transition-colors"
          >
            Stop
          </button>
        </div>
      </div>
    </div>
  );
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
  onRename,
  pane,
  orchestrators,
}: TabItemProps) {
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(tab.title);
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number } | null>(null);
  const [confirmClose, setConfirmClose] = useState(false);

  const handleClose = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    if (hasAgent) {
      setConfirmClose(true);
    } else {
      onClose();
    }
  }, [hasAgent, onClose]);

  return (
    <>
      <div
        onClick={onClick}
        draggable
        onDragStart={(e) => {
          e.dataTransfer.setData("text/plain", JSON.stringify({
            tabId: tab.id,
            fromWorkspaceId: workspaceId,
          }));
          e.dataTransfer.effectAllowed = "move";
        }}
        onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); setCtxMenu({ x: e.clientX, y: e.clientY }); }}
        className={`
          group flex items-center justify-between pl-8 pr-2 py-1 cursor-pointer transition-colors
          ${isActive ? "bg-[var(--ct-active)] text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"}
        `}
      >
        <div className="flex items-center gap-2 min-w-0">
          {/* Icon — role icon > orange sparkle for Claude Code (spins when working) > terminal icon */}
          {pane?.role ? (
            <span
              className={`shrink-0 text-[11px] leading-none ${pane.role === "Orchestrator" || pane.role === "Conductor" ? "text-[var(--ct-yellow)]" : "text-[var(--ct-muted-fg)]"} ${status === "Working" ? "animate-pulse" : ""}`}
              title={`${pane.role}${status === "Working" ? " — working" : ""}`}
            >{ROLE_ICONS[pane.role]}</span>
          ) : hasAgent ? (
            <span
              className={`shrink-0 text-[11px] leading-none text-[#f0883e] ${status === "Working" ? "animate-spin" : ""}`}
              style={status === "Working" ? { animationDuration: "1.5s" } : undefined}
              title={status === "Working" ? "Claude Code working..." : status === "Waiting" ? "Claude Code — waiting for input" : "Claude Code"}
            >&#10035;</span>
          ) : (
            <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor" className="shrink-0">
              <path d="M0 2.75C0 1.784.784 1 1.75 1h12.5c.966 0 1.75.784 1.75 1.75v10.5A1.75 1.75 0 0 1 14.25 15H1.75A1.75 1.75 0 0 1 0 13.25Zm1.75-.25a.25.25 0 0 0-.25.25v10.5c0 .138.112.25.25.25h12.5a.25.25 0 0 0 .25-.25V2.75a.25.25 0 0 0-.25-.25ZM3.5 4.5l3 2.5-3 2.5V4.5ZM8.5 10h3v1.5h-3V10Z" />
            </svg>
          )}
          {editing ? (
            <input
              autoFocus
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
              onClick={(e) => e.stopPropagation()}
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
        {/* Close button — visible on hover */}
        {canClose && (
          <button
            onClick={handleClose}
            className="shrink-0 w-5 h-5 flex items-center justify-center rounded opacity-0 group-hover:opacity-60 hover:!opacity-100 hover:bg-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-all"
            title="Close tab"
          >
            <svg width="7" height="7" viewBox="0 0 10 10" fill="currentColor">
              <path d="M1.41 0L5 3.59 8.59 0 10 1.41 6.41 5 10 8.59 8.59 10 5 6.41 1.41 10 0 8.59 3.59 5 0 1.41Z" />
            </svg>
          </button>
        )}
        {/* Context menu */}
        {ctxMenu && (
          <>
            <div className="fixed inset-0 z-40" onClick={() => setCtxMenu(null)} onContextMenu={(e) => { e.preventDefault(); setCtxMenu(null); }} />
            <div
              className="fixed z-50 w-36 bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-md shadow-lg py-1"
              style={{ left: ctxMenu.x, top: ctxMenu.y }}
            >
              <button
                onClick={() => { setCtxMenu(null); setEditName(tab.title); setEditing(true); }}
                className="flex items-center gap-2 w-full px-3 py-1 text-xs text-[var(--ct-fg)] hover:bg-[var(--ct-border)]"
              >Rename</button>
              {/* Hierarchy is managed via CLI — no manual role assignment in UI */}
              {pane?.role && (
                <button
                  onClick={() => {
                    setCtxMenu(null);
                    if (pane) invoke("mux_clear_pane_role", { paneId: pane.id });
                  }}
                  className="flex items-center gap-2 w-full px-3 py-1 text-xs text-[var(--ct-yellow)] hover:bg-[var(--ct-border)]"
                >Remove Role</button>
              )}
              {pane?.role && (pane.role === "Orchestrator" || pane.role === "Conductor" || pane.role === "Lead") && (
                <button
                  onClick={() => {
                    setCtxMenu(null);
                    if (pane) invoke("mux_set_can_spawn", { paneId: pane.id, canSpawn: !pane.can_spawn });
                  }}
                  className="flex items-center gap-2 w-full px-3 py-1 text-xs text-[var(--ct-fg)] hover:bg-[var(--ct-border)]"
                >{pane.can_spawn ? "Disallow Spawning" : "Allow Spawning"}</button>
              )}
              <div className="h-px bg-[var(--ct-border)] my-0.5" />
              {canClose && (
                <button
                  onClick={(e) => { setCtxMenu(null); handleClose(e); }}
                  className="flex items-center gap-2 w-full px-3 py-1 text-xs text-[var(--ct-red)] hover:bg-[var(--ct-border)]"
                >Close</button>
              )}
            </div>
          </>
        )}
      </div>
      {confirmClose && (
        <ConfirmStopDialog
          onConfirm={() => { onClose(); setConfirmClose(false); }}
          onCancel={() => setConfirmClose(false)}
        />
      )}
    </>
  );
}
