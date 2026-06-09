import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { X, Pencil, UserMinus, MessageSquare, SquareTerminal } from "lucide-react";
import { getAgentProvider } from "../../lib/agentProviders";
import {
  emitPaneUiMode,
  isChatUiSupported,
  maybeAskDefaultNewAgentPaneToChat,
  shouldDefaultNewAgentPaneToChat,
} from "../../lib/chatUiMode";
import {
  getClientPaneUiMode,
  getCurrentPaneUiMode,
  listenClientPaneUiMode,
  setClientPaneUiMode,
} from "../../lib/clientPaneUiMode";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { TelegramIcon, TerminalIcon } from "../Icons";
import { AgentIcon } from "../AgentIcon";
import type { Pane, PaneStatus, Tab } from "../../lib/types";

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
  touchMode?: boolean;
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
  touchMode = false,
}: TabItemProps) {
  const agentProvider = getAgentProvider(pane?.agent_type);
  const isSubPane = Boolean(pane?.parent_pane_id) || (pane?.depth ?? 0) > 0;
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(tab.title);
  // Mirror TerminalPane's mode resolution: no client override falls back to
  // the agent default (claude/GA panes open in chat), so the menu reflects
  // what's actually showing instead of always saying "Open Chat UI".
  const resolveChatView = (p: typeof pane): boolean => {
    if (!p) return false;
    const current = getCurrentPaneUiMode(p.id);
    if (current) return current === "chat";
    const client = getClientPaneUiMode(p.id);
    return client ? client === "chat" : shouldDefaultNewAgentPaneToChat(p.agent_type);
  };
  const [chatView, setChatView] = useState(() => resolveChatView(pane));
  const tabRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!pane) {
      setChatView(false);
      return;
    }
    setChatView(resolveChatView(pane));
    return listenClientPaneUiMode(pane.id, (mode) => setChatView(mode === "chat"));
  }, [pane?.id, pane?.agent_type]);

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
    // Main (the protected GA pane) is the anchor — no rename.
    if (!pane?.protected) {
      items.push({
        label: "Rename",
        icon: <Pencil size={11} strokeWidth={2} />,
        onClick: () => { setEditName(tab.title); setEditing(true); },
      });
    }
    // Role management — hide for Orchestrators (default role on all tabs)
    if (pane?.role && pane.role !== "Orchestrator") {
      items.push({
        label: "Remove Role",
        icon: <UserMinus size={11} strokeWidth={2} />,
        onClick: () => { if (pane) invoke("mux_clear_pane_role", { paneId: pane.id }); },
      });
    }
    if (pane && isChatUiSupported(pane.agent_type)) {
      const nextChatView = !chatView;
      items.push({
        label: nextChatView ? "Open Chat UI" : "Open Terminal",
        icon: nextChatView
          ? <MessageSquare size={11} strokeWidth={2} />
          : <SquareTerminal size={11} strokeWidth={2} />,
        onClick: () => {
          setChatView(nextChatView);
          setClientPaneUiMode(pane.id, nextChatView ? "chat" : "terminal");
          emitPaneUiMode(pane.id, nextChatView ? "chat" : "terminal");
          if (nextChatView) maybeAskDefaultNewAgentPaneToChat();
        },
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
  }, [tab.title, pane, chatView, canClose, hasAgent, onClose]);

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
      data-testid="tab-item"
      data-tab-id={tab.id}
      onClick={onClick}
      draggable={!touchMode}
      onDragStart={(e) => {
        if (touchMode) {
          e.preventDefault();
          return;
        }
        e.dataTransfer.setData("text/plain", JSON.stringify({
          tabId: tab.id,
          fromWorkspaceId: workspaceId,
        }));
        e.dataTransfer.effectAllowed = "move";
      }}
      className={`
        sidebar-tab group flex items-center justify-between cursor-pointer transition-colors relative
        ${touchMode ? "sidebar-tab-touch" : ""}
        ${touchMode ? "min-h-11 pl-5 pr-2 py-2" : "pl-4 pr-2 py-1.5"}
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
      <div className={`flex items-center min-w-0 relative z-[1] ${touchMode ? "gap-2.5" : "gap-2"}`}>
        {hasAgent ? (
          <AgentIcon
            // Pass agent_type directly — AgentIcon resolves via the backend descriptor cache (icon_svg ships with every registered provider, JS-plugin or
            // native). Going through the FE-only getAgentProvider list dropped any third-party drop-in plugin to a "claude" fallback icon.
            providerId={pane?.agent_type ?? "claude"}
            working={status === "Working"}
            className={`shrink-0 leading-none ${touchMode ? "text-[15px]" : isSubPane ? "text-[10px]" : "text-[12px]"}`}
            title={status === "Working" ? `${agentProvider?.displayName || "Agent"} working...` : status === "Waiting" ? `${agentProvider?.displayName || "Agent"} — waiting for input` : agentProvider?.displayName || "Agent"}
          />
        ) : (
          <TerminalIcon className={`shrink-0 ${touchMode ? "scale-110" : isSubPane ? "scale-90" : ""}`} />
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
            className={`${touchMode ? "text-[14px]" : "text-xs"} truncate ${isActive ? "font-medium" : ""}`}
            onDoubleClick={(e) => {
              if (pane?.protected) return; // Main is unrenameable
              e.stopPropagation();
              setEditName(tab.title);
              setEditing(true);
            }}
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
      {/* Right slot: model badge by default, 3-dot menu on hover (layered) */}
      <div className="shrink-0 relative z-[1] flex items-center">
        {pane?.agent_session_model && !touchMode && (
          <span
            className="sidebar-tab-badge px-1 py-0 text-[8px] leading-tight rounded font-mono uppercase tracking-wider border border-[var(--ct-border)] text-[var(--ct-muted-fg)]"
            title={pane.agent_session_model}
          >
            {pane.agent_session_model}
          </span>
        )}
        {menuItems.length > 0 && (
          <DropdownMenu
            items={menuItems}
            className={
              touchMode
                ? "flex h-11 w-11 items-center justify-center"
                : "sidebar-tab-close absolute inset-y-0 right-0 flex items-center"
            }
            trigger={
              <button
                className={`${touchMode ? "h-11 w-11 text-[var(--ct-muted-fg)]" : "w-5 h-5 text-[var(--ct-muted-fg)]"} flex items-center justify-center rounded hover:text-[var(--ct-fg)] hover:bg-[var(--ct-border)] transition-colors`}
                title="Tab actions"
              >
                <span className={`${touchMode ? "text-[20px]" : "text-[13px]"} leading-none font-bold`}>⋮</span>
              </button>
            }
          />
        )}
      </div>
    </div>
  );
}
