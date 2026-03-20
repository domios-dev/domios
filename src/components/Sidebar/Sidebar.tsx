import { useCallback, useMemo, useState } from "react";
import { useMultiplexer } from "../../store/multiplexer";
import { useGitInfo } from "../../hooks/useGitInfo";
import { NotificationBadge } from "../Notifications/NotificationBadge";
import { NotificationPanel } from "../Notifications/NotificationPanel";
import type { Workspace, Tab, SplitNode, PaneStatus } from "../../lib/types";

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
    markRead,
    markAllRead,
    clearNotifications,
    setFocusedPane,
  } = useMultiplexer();

  const [panelOpen, setPanelOpen] = useState(false);
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Set<string>>(
    () => new Set(state.active_workspace_id ? [state.active_workspace_id] : []),
  );

  // Auto-expand active workspace when it changes
  useMemo(() => {
    if (state.active_workspace_id) {
      setExpandedWorkspaces((prev) => {
        const next = new Set(prev);
        next.add(state.active_workspace_id!);
        return next;
      });
    }
  }, [state.active_workspace_id]);

  const gitInfo = useGitInfo(state.focused_pane_id);

  const unreadCount = useMemo(
    () => state.notifications.filter((n) => !n.read).length,
    [state.notifications],
  );

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

  const handleJumpToPane = useCallback(
    (paneId: string, workspaceId: string | null) => {
      if (workspaceId) {
        setActiveWorkspace(workspaceId);
      }
      setFocusedPane(paneId);
      setPanelOpen(false);
    },
    [setActiveWorkspace, setFocusedPane],
  );

  return (
    <>
      <aside className="w-[220px] shrink-0 bg-[#161b22] border-r border-[#30363d] flex flex-col select-none overflow-hidden">
        {/* Workspaces header */}
        <div className="flex items-center justify-between px-3 py-2 border-b border-[#30363d]">
          <span className="text-xs font-semibold uppercase tracking-wider text-[#8b949e]">
            Workspaces
          </span>
          <div className="flex items-center gap-1">
            <div className="relative">
              <button
                onClick={() => setDropdownOpen(v => !v)}
                className="flex items-center justify-center w-5 h-5 rounded hover:bg-[#30363d] text-[#8b949e] hover:text-[#c9d1d9] transition-colors"
                title="New..."
              >
                <svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor">
                  <path d="M6 1v10M1 6h10" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" />
                </svg>
              </button>
              {dropdownOpen && (
                <>
                  <div className="fixed inset-0 z-40" onClick={() => setDropdownOpen(false)} />
                  <div className="absolute right-0 top-full mt-1 z-50 w-48 bg-[#1c2128] border border-[#30363d] rounded-md shadow-lg py-1">
                    <button
                      onClick={() => {
                        setDropdownOpen(false);
                        if (state.active_workspace_id) {
                          const ws = state.workspaces.find(w => w.id === state.active_workspace_id);
                          if (ws) createTab(ws.id);
                        }
                      }}
                      className="flex items-center gap-2 w-full px-3 py-1.5 text-xs text-[#c9d1d9] hover:bg-[#30363d] transition-colors"
                    >
                      <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
                        <path d="M0 2.75C0 1.784.784 1 1.75 1h12.5c.966 0 1.75.784 1.75 1.75v10.5A1.75 1.75 0 0 1 14.25 15H1.75A1.75 1.75 0 0 1 0 13.25Zm1.75-.25a.25.25 0 0 0-.25.25v10.5c0 .138.112.25.25.25h12.5a.25.25 0 0 0 .25-.25V2.75a.25.25 0 0 0-.25-.25ZM3.5 4.5l3 2.5-3 2.5V4.5ZM8.5 10h3v1.5h-3V10Z" />
                      </svg>
                      <span>New Tab</span>
                      <span className="ml-auto text-[10px] text-[#484f58]">Ctrl+Shift+T</span>
                    </button>
                    <button
                      onClick={() => { setDropdownOpen(false); handleNewWorkspace(); }}
                      className="flex items-center gap-2 w-full px-3 py-1.5 text-xs text-[#c9d1d9] hover:bg-[#30363d] transition-colors"
                    >
                      <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
                        <path d="M1.75 1A1.75 1.75 0 0 0 0 2.75v10.5C0 14.216.784 15 1.75 15h12.5A1.75 1.75 0 0 0 16 13.25v-8.5A1.75 1.75 0 0 0 14.25 3H7.5a.25.25 0 0 1-.2-.1l-.9-1.2C6.07 1.26 5.55 1 5 1H1.75Z" />
                      </svg>
                      <span>New Workspace</span>
                      <span className="ml-auto text-[10px] text-[#484f58]">Ctrl+Shift+N</span>
                    </button>
                  </div>
                </>
              )}
            </div>
          </div>
        </div>

        {/* Git branch info */}
        {gitInfo?.branch && (
          <div className="px-3 py-1 border-b border-[#30363d] flex items-center gap-1.5 text-[10px] text-[#8b949e]">
            <span className="opacity-70">{"\u2387"}</span>
            <span className="truncate">{gitInfo.branch}</span>
            {gitInfo.is_dirty && (
              <span className="inline-block w-1.5 h-1.5 rounded-full bg-[#d29922] shrink-0" title="Uncommitted changes" />
            )}
          </div>
        )}

        {/* Workspace tree */}
        <div className="flex-1 overflow-y-auto">
          {state.workspaces.map((ws) => {
            const allPaneIds = ws.tabs.flatMap((t) => collectPaneIds(t.split_tree));
            const wsStatus = aggregateStatus(allPaneIds, state.paneStatuses);
            const hasWaiting = allPaneIds.some(
              (pid) => state.paneStatuses[pid] === "Waiting",
            );
            const isActive = ws.id === state.active_workspace_id;
            const isExpanded = expandedWorkspaces.has(ws.id);

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
                  canClose={state.workspaces.length > 1}
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
                {isExpanded && (
                  <>
                    {ws.tabs.map((tab, index) => (
                      <TabItem
                        key={tab.id}
                        tab={tab}
                        index={index}
                        isActive={isActive && tab.id === ws.active_tab_id}
                        onClick={() => {
                          setActiveWorkspace(ws.id);
                          setActiveTab(ws.id, tab.id);
                        }}
                        onClose={() => closeTab(ws.id, tab.id)}
                        canClose={ws.tabs.length > 1}
                        paneCount={collectPaneIds(tab.split_tree).length}
                        workspaceId={ws.id}
                        hasAgent={collectPaneIds(tab.split_tree).some(
                          (pid) => state.panes[pid]?.agent_type === "claude"
                        )}
                        status={aggregateStatus(collectPaneIds(tab.split_tree), state.paneStatuses)}
                      />
                    ))}
                    {/* Add tab button inside folder */}
                    <button
                      onClick={() => createTab(ws.id)}
                      className="flex items-center gap-2 w-full pl-8 pr-3 py-1 text-[#484f58] hover:text-[#8b949e] hover:bg-[#1c2128] transition-colors"
                      title="New tab in this workspace"
                    >
                      <svg width="10" height="10" viewBox="0 0 12 12" fill="none">
                        <path d="M6 1v10M1 6h10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
                      </svg>
                      <span className="text-[10px]">New tab</span>
                    </button>
                  </>
                )}
              </div>
            );
          })}
        </div>

        {/* Settings button at bottom */}
        <div className="border-t border-[#30363d] px-3 py-2">
          <button
            onClick={onOpenSettings}
            className="flex items-center gap-2 w-full px-2 py-1.5 rounded text-[#8b949e] hover:bg-[#1c2128] hover:text-[#c9d1d9] transition-colors"
            title="Settings"
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" className="shrink-0">
              <path d="M8 0a8.2 8.2 0 0 1 .701.031C9.444.095 9.99.645 10.16 1.29l.288 1.107c.018.066.079.158.212.224.231.114.454.243.668.386.123.082.233.09.3.071l1.102-.303c.644-.176 1.392.021 1.82.63.27.385.506.792.704 1.218.315.675.111 1.422-.364 1.891l-.814.806c-.049.048-.098.147-.088.294a6.57 6.57 0 0 1 0 .772c-.01.147.04.246.088.294l.814.806c.475.469.679 1.216.364 1.891a7.977 7.977 0 0 1-.704 1.217c-.428.61-1.176.807-1.82.63l-1.103-.303c-.066-.019-.176-.011-.299.071a5.909 5.909 0 0 1-.668.386c-.133.066-.194.158-.212.224l-.288 1.107c-.17.645-.716 1.195-1.459 1.26a8.006 8.006 0 0 1-1.402 0c-.743-.065-1.289-.615-1.459-1.26l-.289-1.107c-.017-.066-.078-.158-.211-.224a5.909 5.909 0 0 1-.668-.386c-.123-.082-.233-.09-.3-.071l-1.102.303c-.644.176-1.392-.021-1.82-.63a8.012 8.012 0 0 1-.704-1.218c-.315-.675-.111-1.422.363-1.891l.815-.806c.049-.048.098-.147.088-.294a6.57 6.57 0 0 1 0-.772c.01-.147-.04-.246-.088-.294l-.815-.806C.635 6.045.431 5.298.746 4.623a7.92 7.92 0 0 1 .704-1.217c.428-.61 1.176-.807 1.82-.63l1.103.303c.066.019.176.011.299-.071.214-.143.437-.272.668-.386.133-.066.194-.158.212-.224L5.84 1.29c.17-.645.716-1.195 1.459-1.26A8.2 8.2 0 0 1 8 0ZM5.5 8a2.5 2.5 0 1 0 5 0 2.5 2.5 0 0 0-5 0Z" />
            </svg>
            <span className="text-xs">Settings</span>
          </button>
        </div>
      </aside>

    </>
  );
}

// ---------------------------------------------------------------------------
// Status dot component
// ---------------------------------------------------------------------------

const STATUS_COLORS: Record<PaneStatus, string> = {
  Working: "#7ee787",
  Waiting: "#d29922",
  Idle: "#484f58",
  Dead: "#f85149",
};

function StatusDot({ status, animate }: { status: PaneStatus | null; animate?: boolean }) {
  if (!status) return null;
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
  canClose,
  status,
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

  return (
    <div
      className={`
        group flex items-center justify-between px-1 py-1.5 cursor-pointer transition-colors
        ${isActive ? "bg-[#1f2937] text-[#c9d1d9]" : "text-[#8b949e] hover:bg-[#1c2128] hover:text-[#c9d1d9]"}
        ${hasWaiting ? "bg-[#d29922]/10" : ""}
        ${dragOver ? "bg-[#1f6feb]/20" : ""}
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
        {/* Chevron toggle */}
        <button
          onClick={(e) => {
            e.stopPropagation();
            onToggleExpand();
          }}
          className="flex items-center justify-center w-4 h-4 shrink-0 text-[#484f58] hover:text-[#8b949e] transition-colors"
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
              className="text-xs bg-[#0d1117] text-[#c9d1d9] border border-[#30363d] rounded px-1 py-0 w-full outline-none"
            />
          ) : (
            <span className="text-xs truncate" onDoubleClick={handleDoubleClick}>
              {workspace.name}
            </span>
          )}
        </div>
        <span className="text-[10px] text-[#484f58]">
          {workspace.tabs.length}
        </span>
        {!isExpanded && <StatusDot status={status} animate />}
        {hasWaiting && (
          <span className="inline-flex items-center justify-center min-w-[14px] h-3.5 px-0.5 rounded-full bg-[#d29922]/20 text-[#d29922] text-[9px] font-bold leading-none">
            !
          </span>
        )}
      </div>
      {canClose && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onClose();
          }}
          className="hidden group-hover:flex items-center justify-center w-4 h-4 rounded hover:bg-[#30363d] text-[#484f58] hover:text-[#c9d1d9] transition-colors mr-1"
          title="Close workspace"
        >
          <svg width="8" height="8" viewBox="0 0 10 10" fill="currentColor">
            <path d="M1.41 0L5 3.59 8.59 0 10 1.41 6.41 5 10 8.59 8.59 10 5 6.41 1.41 10 0 8.59 3.59 5 0 1.41Z" />
          </svg>
        </button>
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
}: TabItemProps) {
  return (
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
      className={`
        group flex items-center justify-between pl-8 pr-3 py-1 cursor-pointer transition-colors
        ${isActive ? "bg-[#1f2937] text-[#c9d1d9] border-l-2 border-[#58a6ff]" : "text-[#8b949e] hover:bg-[#1c2128] hover:text-[#c9d1d9] border-l-2 border-transparent"}
      `}
    >
      <div className="flex items-center gap-2 min-w-0">
        {/* Icon — orange sparkle for Claude Code (spins when working), terminal icon otherwise */}
        {hasAgent ? (
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
        <span className="text-xs truncate">
          {hasAgent ? tab.title.replace(/^[✻✳✴✵✶✷✸✹✺✻✼✽✾✿❀❁❂❃❇❈❉❊❋*]\s*/u, "") : tab.title}
        </span>
        {paneCount > 1 && (
          <span className="text-[10px] text-[#484f58]">[{paneCount}]</span>
        )}
      </div>
      {canClose && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onClose();
          }}
          className="hidden group-hover:flex items-center justify-center w-4 h-4 rounded hover:bg-[#30363d] text-[#484f58] hover:text-[#c9d1d9] transition-colors"
          title="Close tab (Ctrl+Shift+W)"
        >
          <svg width="8" height="8" viewBox="0 0 10 10" fill="currentColor">
            <path d="M1.41 0L5 3.59 8.59 0 10 1.41 6.41 5 10 8.59 8.59 10 5 6.41 1.41 10 0 8.59 3.59 5 0 1.41Z" />
          </svg>
        </button>
      )}
    </div>
  );
}
