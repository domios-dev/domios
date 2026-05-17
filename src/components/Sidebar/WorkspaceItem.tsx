import { useCallback, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X, TerminalSquare, FolderOpen } from "lucide-react";
import { isMac } from "../../lib/keybindings";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useConfig } from "../../hooks/useConfig";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { FolderIcon, ShieldCheckIcon } from "../Icons";
import { AgentIcon } from "../AgentIcon";
import type { PaneStatus, Workspace } from "../../lib/types";

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

export function WorkspaceItem({
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

  const { config } = useConfig();
  const agentsConfig = config.agents ?? {};
  // Backend descriptors — single source of truth across bundled +
  // user-installed JS plugins. Frontend `getAllProviders()` only knew
  // about plugins with a `src/index.ts` FE module, which left
  // drop-in `~/.codeterm/plugins/<id>/plugin.js` plugins invisible
  // in the workspace menu.
  const { providers } = useAgentProviders();

  const menuItems: DropdownMenuItem[] = useMemo(() => {
    const items: DropdownMenuItem[] = [
      {
        label: "New Terminal",
        icon: <TerminalSquare size={11} strokeWidth={1.5} />,
        onClick: onNewTerminal,
      },
    ];
    for (const provider of providers) {
      if (agentsConfig[provider.id]?.visible_in_menu === false) continue;
      items.push({
        label: `New ${provider.display_name} Session`,
        icon: <AgentIcon providerId={provider.id} className="text-[12px]" />,
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
  }, [onNewTerminal, onNewAgent, onClose, workspace.protected, workspace.cwd, agentsConfig, providers]);

  return (
    <div
      className={`
        group flex items-center justify-between px-1 py-2 cursor-pointer transition-colors
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
            <span className="text-[13px] leading-none font-bold">⋮</span>
          </button>
        }
      />
    </div>
  );
}
