import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X, Pencil, UserMinus, Ban, CircleCheck } from "lucide-react";
import { getAgentProvider } from "../../lib/agentProviders";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { TelegramIcon, TerminalIcon } from "../Icons";
import { AgentIcon } from "../AgentIcon";
import type { Pane, PaneStatus, Tab } from "../../lib/types";
import { ROLE_ICONS } from "./HierarchyTree";

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

export function TabItem({
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
          <AgentIcon
            providerId={agentProvider?.id ?? "claude"}
            working={status === "Working"}
            className="shrink-0 text-[12px] leading-none"
            title={status === "Working" ? `${agentProvider?.displayName || "Agent"} working...` : status === "Waiting" ? `${agentProvider?.displayName || "Agent"} — waiting for input` : agentProvider?.displayName || "Agent"}
          />
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
        {pane?.agent_session_model && (
          <span
            className="shrink-0 px-1 py-0 text-[9px] leading-tight rounded font-mono uppercase tracking-wider border border-[var(--ct-border)] text-[var(--ct-muted-fg)]"
            title={pane.agent_session_model}
          >
            {pane.agent_session_model}
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
              <span className="text-[13px] leading-none font-bold">⋮</span>
            </button>
          }
        />
      )}
    </div>
  );
}
