import { memo, useCallback, useMemo, useState } from "react";
import { invoke } from "../../lib/bridge";
import { X, TerminalSquare, FolderOpen, Sparkles, SquarePlus } from "lucide-react";
import { isMac } from "../../lib/keybindings";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useConfig } from "../../hooks/useConfig";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { FolderIcon, ShieldCheckIcon } from "../Icons";
import { AgentIcon } from "../AgentIcon";
import { SidebarDisclosure } from "./SidebarDisclosure";
import type { TabStatus, Workspace } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";

interface WorkspaceItemProps {
  workspace: Workspace;
  isActive: boolean;
  isExpanded: boolean;
  onClick: () => void;
  onToggleExpand: () => void;
  onClose: () => void;
  onRename: (name: string) => void;
  onNewTerminal: () => void;
  onNewSession: () => void;
  newSessionDisabled?: boolean;
  onNewAgent: (agentId: string) => void;
  canClose: boolean;
  status: TabStatus | null;
  showDisclosure?: boolean;
  detailHeader?: boolean;
  touchMode?: boolean;
  uiFontScale?: number;
}

function WorkspaceItemInner({
  workspace,
  isActive,
  isExpanded,
  onClick,
  onToggleExpand,
  onClose,
  onRename,
  onNewTerminal,
  onNewSession,
  newSessionDisabled = false,
  onNewAgent,
  canClose: _canClose,
  status: _status,
  showDisclosure = true,
  detailHeader = false,
  touchMode = false,
  uiFontScale,
}: WorkspaceItemProps) {
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(workspace.name);

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

  const { config } = useConfig();
  const agentsConfig = config.agents ?? {};
  // plugins with a `src/index.ts` FE module, which left drop-in `~/.codeterm/plugins/<id>/plugin.js` plugins invisible in the workspace menu.
  const { providers } = useAgentProviders();
  const workspaceText = pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const workspaceTouchText = pickUiFontScaleValue(uiFontScale, ["text-[13px]", "text-[14px]", "text-[15px]", "text-base"]);
  const workspaceCountText = pickUiFontScaleValue(uiFontScale, ["text-[9px]", "text-[10px]", "text-[11px]", "text-xs"]);

  const menuItems: DropdownMenuItem[] = useMemo(() => {
    const quickProviders = [
      { id: "codex", label: "Codex" },
      { id: "claude", label: "Claude Code" },
      { id: "opencode", label: "OpenCode" },
      { id: "cursor", label: "Cursor" },
    ].map(({ id, label }) => {
      const provider = providers.find((candidate) => candidate.id === id);
      const visible = agentsConfig[id]?.visible_in_menu !== false;
      const available = Boolean(provider && visible && !newSessionDisabled);
      return {
        label,
        icon: <AgentIcon providerId={id} className="text-[12px]" />,
        disabled: !available,
        title: newSessionDisabled
          ? "Finish Get started before creating another session"
          : !provider || !visible
            ? `${label} is unavailable`
            : undefined,
        onClick: available ? () => onNewAgent(id) : undefined,
        "data-testid": `workspace-quick-spawn-${id}`,
      } satisfies DropdownMenuItem;
    });
    const items: DropdownMenuItem[] = [
      {
        label: "New session",
        icon: <SquarePlus size={11} strokeWidth={1.6} />,
        disabled: newSessionDisabled,
        title: newSessionDisabled ? "Finish Get started before creating another session" : undefined,
        onClick: newSessionDisabled ? undefined : onNewSession,
        "data-testid": "workspace-new-session",
      },
      {
        label: "Quick spawn",
        icon: <Sparkles size={11} strokeWidth={1.6} />,
        disabled: newSessionDisabled,
        title: newSessionDisabled ? "Finish Get started before creating another session" : undefined,
        submenu: {
          label: "Quick spawn",
          description: "Spawns a default Worker",
          items: quickProviders,
        },
        "data-testid": "workspace-quick-spawn",
      },
      {
        label: "New Terminal",
        icon: <TerminalSquare size={11} strokeWidth={1.5} />,
        onClick: onNewTerminal,
      },
    ];
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
        label: "Close Workspace",
        icon: <X size={11} strokeWidth={2} />,
        onClick: onClose,
        danger: true,
      });
    }
    return items;
  }, [onNewTerminal, onNewSession, newSessionDisabled, onNewAgent, onClose, workspace.protected, workspace.cwd, agentsConfig, providers]);

  return (
    <div
      data-testid="workspace-item"
      data-workspace-id={workspace.id}
      data-workspace-name={workspace.name}
      data-variant={detailHeader ? "detail-header" : "tree-row"}
      className={`
        ${detailHeader ? "sidebar-workspace-detail-header" : "sidebar-workspace-row"} group relative flex items-center justify-between cursor-pointer transition-colors
        ${touchMode ? "min-h-11 px-2 py-2" : "px-1 py-2"}
        ${detailHeader ? "text-[var(--ct-fg)]" : isActive ? "bg-[var(--ct-active)] text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"}
      `}
    >
      <div className={`flex items-center min-w-0 ${touchMode ? "gap-2" : "gap-1"}`}>
        {showDisclosure && (
          <SidebarDisclosure
            expanded={isExpanded}
            touchMode={touchMode}
            onClick={(e) => {
              e.stopPropagation();
              onToggleExpand();
            }}
            className="shrink-0"
            title={isExpanded ? "Collapse" : "Expand"}
          />
        )}
        {/* Tree rows activate; detail headers identify the selected workspace. */}
        {editing ? (
          <div className={`flex items-center min-w-0 ${touchMode ? "gap-2.5" : "gap-2"}`}>
            {!detailHeader && (workspace.protected ? (
              <span className="relative shrink-0 flex items-center justify-center w-[14px] h-[14px]">
                <ShieldCheckIcon style={{ stroke: "var(--ct-muted-fg)" }} className="shrink-0" />
              </span>
            ) : (
              <FolderIcon open={isExpanded} className="shrink-0" />
            ))}
            <input
              autoFocus
              aria-label={`Rename workspace ${workspace.name}`}
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
              onBlur={commitRename}
              onKeyDown={handleKeyDown}
              onClick={(e) => e.stopPropagation()}
              className={`${touchMode ? `${workspaceTouchText} py-1` : `${workspaceText} py-0`} bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded px-1 w-full outline-none`}
            />
          </div>
        ) : detailHeader ? (
          <div
            className={`flex items-center min-w-0 ${touchMode ? "gap-2.5" : "gap-2"}`}
            onClick={onClick}
          >
            <span
              className={`${touchMode ? workspaceTouchText : workspaceText} font-medium truncate`}
              onDoubleClick={handleDoubleClick}
              title={workspace.cwd || undefined}
              role="heading"
              aria-level={3}
            >
              {workspace.name}
            </span>
          </div>
        ) : (
          <button
            type="button"
            aria-label={`Open workspace ${workspace.name}`}
            className={`flex items-center min-w-0 bg-transparent border-0 p-0 text-inherit font-inherit cursor-pointer ${touchMode ? "gap-2.5" : "gap-2"}`}
            onClick={onClick}
          >
            {workspace.protected ? (
              <span className="relative shrink-0 flex items-center justify-center w-[14px] h-[14px]">
                <ShieldCheckIcon style={{ stroke: "var(--ct-muted-fg)" }} className="shrink-0" />
              </span>
            ) : (
              <FolderIcon open={isExpanded} className="shrink-0" />
            )}
            <span className={`${touchMode ? workspaceTouchText : workspaceText} font-medium truncate`} onDoubleClick={handleDoubleClick} title={workspace.cwd || undefined}>
              {workspace.name}
            </span>
          </button>
        )}
        <span className={`${workspaceCountText} text-[var(--ct-muted)]`}>
          {workspace.tabs.length}
        </span>
      </div>
      {/* 3-dot menu — visible on hover */}
      <DropdownMenu
        items={menuItems}
        className={
          touchMode
            ? "shrink-0 flex h-11 w-11 items-center justify-center"
            : "shrink-0 opacity-0 group-hover:opacity-60 hover:!opacity-100 transition-opacity"
        }
        trigger={
          <button
            aria-label={`Actions for ${workspace.name}`}
            className={`${touchMode ? "h-11 w-11" : "w-5 h-5"} flex items-center justify-center rounded hover:bg-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors`}
            title="Workspace actions"
          >
            <span className={`${touchMode ? "text-[20px]" : "text-[13px]"} leading-none font-bold`}>⋮</span>
          </button>
        }
      />
    </div>
  );
}

function workspacePropsEqual(prev: WorkspaceItemProps, next: WorkspaceItemProps): boolean {
  return (
    prev.isActive === next.isActive &&
    prev.isExpanded === next.isExpanded &&
    prev.status === next.status &&
    prev.canClose === next.canClose &&
    prev.showDisclosure === next.showDisclosure &&
    prev.detailHeader === next.detailHeader &&
    prev.touchMode === next.touchMode &&
    prev.uiFontScale === next.uiFontScale &&
    prev.workspace.id === next.workspace.id &&
    prev.workspace.name === next.workspace.name &&
    prev.workspace.cwd === next.workspace.cwd &&
    prev.workspace.tabs.length === next.workspace.tabs.length
  );
}

export const WorkspaceItem = memo(WorkspaceItemInner, workspacePropsEqual);
