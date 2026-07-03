import { memo, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { X, Pencil, UserMinus, MessageSquare, SquareTerminal, UserCog } from "lucide-react";
import {
  emitPaneUiMode,
  isChatUiSupported,
  shouldDefaultNewAgentPaneToChat,
} from "../../lib/chatUiMode";
import { usePaneProvider, useShellSessionModel } from "../../lib/usePaneProvider";
import {
  getClientPaneUiMode,
  getCurrentPaneUiMode,
  listenCurrentPaneUiMode,
  setClientPaneUiMode,
} from "../../lib/clientPaneUiMode";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { TelegramIcon, TerminalIcon } from "../Icons";
import { ProviderGlyph } from "../ProviderGlyph";
import type { Pane, PaneStatus, Tab } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { formatModelBadge } from "../../lib/formatModelBadge";
import { SidebarModelBadge } from "./SidebarModelBadge";
import { TabActivityIndicator } from "./TabActivityIndicator";
import { TabStatusRail } from "./TabStatusRail";
import { RoleIcon } from "./HierarchyTree";
import type { PaneTabActivity } from "../Canvas/paneTabActivity";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { canMakeOrchestrator, makeOrchestrator } from "../../lib/orchestratorOnboard";

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
  uiFontScale?: number;
  /** Plan-membership glance (resolved from the cheap index gate by the Sidebar). */
  activity?: PaneTabActivity | null;
}

function TabItemInner({
  tab,
  isActive,
  onClick,
  onClose,
  canClose,
  paneCount,
  workspaceId: _workspaceId,
  hasAgent,
  status,
  onRename: _onRename,
  pane,
  orchestrators: _orchestrators,
  isAppearing,
  isClosing,
  touchMode = false,
  uiFontScale,
  activity,
}: TabItemProps) {
  // Unified identity for agent (agent_type) AND chatBackend shell (backing.plugin_id)
  // panes — drives the icon, tooltip, and (for shells) the model badge.
  const provider = usePaneProvider(pane);
  const shellSessionModel = useShellSessionModel(pane);
  const providerName = provider?.displayName || "Agent";
  // Agent panes carry the model on the pane record; shell panes don't — fall
  // back to the live chatBackend session model (fetched/cached out of band).
  const tabModel = pane?.agent_session_model ?? shellSessionModel;
  const tabModelBadge = tabModel ? formatModelBadge(tabModel) : null;
  const description = pane?.description ?? tab.description ?? null;
  const canonicalTitle = displayTitleForTab(tab, pane);
  const displayTitle = hasAgent ? canonicalTitle.replace(/^[✻✳✴✵✶✷✸✹✺✻✼✽✾✿❀❁❂❃❇❈❉❊❋*]\s*/u, "") : canonicalTitle;
  const isSubPane = Boolean(pane?.parent_pane_id) || (pane?.depth ?? 0) > 0;
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(canonicalTitle);
  // Mirror TerminalPane's mode resolution: no client override falls back to
  // the agent default (claude/GA panes open in chat), so the menu reflects
  const resolveChatView = (p: typeof pane): boolean => {
    if (!p) return false;
    const current = getCurrentPaneUiMode(p.id);
    if (current) return current === "chat";
    const client = getClientPaneUiMode(p.id);
    return client ? client === "chat" : p.chat_view === true || shouldDefaultNewAgentPaneToChat(p.agent_type, p.backing);
  };
  const [chatView, setChatView] = useState(() => resolveChatView(pane));
  const tabRef = useRef<HTMLDivElement>(null);
  const tabText = pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const tabTouchText = pickUiFontScaleValue(uiFontScale, ["text-[13px]", "text-[14px]", "text-[15px]", "text-base"]);

  useEffect(() => {
    if (!pane) {
      setChatView(false);
      return;
    }
    setChatView(resolveChatView(pane));
    return listenCurrentPaneUiMode(pane.id, (mode) => setChatView(mode === "chat"));
  }, [pane?.id, pane?.agent_type, pane?.backing?.kind]);

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
        onClick: () => { setEditName(canonicalTitle); setEditing(true); },
      });
    }
    if (pane && canMakeOrchestrator(pane)) {
      items.push({
        label: "Make Orchestrator",
        icon: <UserCog size={11} strokeWidth={2} />,
        onClick: () => { void makeOrchestrator(pane.id); },
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
    if (pane && isChatUiSupported(pane.agent_type, pane.backing)) {
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
  }, [canonicalTitle, pane, chatView, canClose, hasAgent, onClose]);

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
      data-tab-no-task-bubble="true"
      title={description ? `${displayTitle}\n${description}` : displayTitle}
      onClick={onClick}
      className={`
        sidebar-tab group flex items-center justify-between cursor-pointer transition-colors relative
        ${touchMode ? "sidebar-tab-touch" : ""}
        ${touchMode ? "min-h-11 pl-5 pr-2 py-2" : "pl-4 pr-2 py-1.5"}
        ${animClass}
        ${isActive ? "text-[var(--ct-fg)]" : "sidebar-tab-dimmed hover:bg-[rgba(255,255,255,0.04)]"}
      `}
    >
      <TabStatusRail
        status={status}
        attention={status === "Waiting" || Boolean(pane?.pending_prompt)}
      />
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
      <div className={`flex flex-1 items-center min-w-0 relative z-[1] ${touchMode ? "gap-2.5" : "gap-2"}`}>
        {provider ? (
          // Agent (descriptor) OR chatBackend shell (plugin snapshot) — both
          // resolve through usePaneProvider, so shell panes get their plugin
          <ProviderGlyph
            iconSvg={provider.iconSvg}
            iconColor={provider.iconColor}
            iconHint={provider.iconHint}
            className={`shrink-0 leading-none ${touchMode ? "text-[15px]" : isSubPane ? "text-[10px]" : "text-[12px]"}`}
            title={status === "Working" ? `${providerName} working...` : status === "Waiting" ? `${providerName} — waiting for input` : providerName}
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
              if (trimmed && trimmed !== canonicalTitle && _onRename) _onRename(trimmed);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                (e.target as HTMLInputElement).blur();
              } else if (e.key === "Escape") {
                setEditName(canonicalTitle);
                setEditing(false);
              }
            }}
            className={`${tabText} bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded px-1 py-0 w-full outline-none`}
          />
        ) : (
          <span className="min-w-0 flex-1">
            <span
              className={`${touchMode ? tabTouchText : tabText} block truncate ${isActive ? "font-medium" : ""}`}
              onDoubleClick={(e) => {
                if (pane?.protected) return; // Main is unrenameable
                e.stopPropagation();
                setEditName(canonicalTitle);
                setEditing(true);
              }}
            >
              {displayTitle}
            </span>
            {description && !activity && (
              <span className="block truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]">
                {description}
              </span>
            )}
          </span>
        )}
        {/* Explicit role (pane.role) stays visible even when plan activity is shown */}
        {pane?.role && !editing && (
          <span data-testid="tab-role-indicator" data-tab-role={pane.role} className="shrink-0 text-[var(--ct-muted-fg)]/70">
            <RoleIcon role={pane.role} uiFontScale={uiFontScale} />
          </span>
        )}
        {activity && pane && !editing && (
          <TabActivityIndicator activity={activity} paneId={pane.id} />
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
            className="shrink-0 w-2 h-2 rounded-full"
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
      {/* Right slot: model badge by default, 3-dot menu on hover (layered).
          The menu is absolutely positioned, so NO overflow-hidden here (it would
          clip the menu when there's no badge to size the slot). Badge truncation
          lives on .sidebar-tab-badge (its own max-width + ellipsis). */}
      <div className="shrink-0 relative z-[1] flex items-center min-w-5 justify-end">
        {tabModelBadge && !touchMode && (
          <SidebarModelBadge label={tabModelBadge} title={tabModel ?? undefined} />
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

// Pane objects are always new references after SET_STATE (reducer spreads panes
// map). Comparing by value for the fields TabItem actually renders prevents
function backingPluginId(p: Pane): string | undefined {
  return p.backing?.kind === "chat_backend" ? p.backing.plugin_id : undefined;
}
function backingSessionId(p: Pane): string | undefined {
  return p.backing?.kind === "chat_backend" ? p.backing.session_id : undefined;
}

function tabItemPropsEqual(prev: TabItemProps, next: TabItemProps): boolean {
  if (
    prev.isActive !== next.isActive ||
    prev.status !== next.status ||
    prev.canClose !== next.canClose ||
    prev.paneCount !== next.paneCount ||
    prev.hasAgent !== next.hasAgent ||
    prev.isAppearing !== next.isAppearing ||
    prev.isClosing !== next.isClosing ||
    prev.touchMode !== next.touchMode ||
    prev.uiFontScale !== next.uiFontScale ||
    prev.index !== next.index ||
    prev.tab.id !== next.tab.id ||
    prev.tab.description !== next.tab.description
  ) return false;

  // Plan-membership glance is a fresh object each Sidebar render — compare by value
  // so a non-task tab (both null) and an unchanged indicator skip the re-render.
  const pa = prev.activity;
  const na = next.activity;
  if (!!pa !== !!na) return false;
  if (pa && na && (
    pa.kind !== na.kind ||
    pa.planId !== na.planId ||
    pa.planTitle !== na.planTitle ||
    pa.taskCount !== na.taskCount ||
    pa.planCount !== na.planCount
  )) return false;

  const pp = prev.pane;
  const np = next.pane;
  if (pp === np) return true;
  if (!pp || !np) return false;
  return (
    pp.id === np.id &&
    // pane.title is the sole title source; omitting these left the tab painting
    // a stale label after wake/cache-reconcile.
    pp.title === np.title &&
    pp.title_level === np.title_level &&
    pp.description === np.description &&
    pp.agent_type === np.agent_type &&
    // backing drives the shell icon/tooltip/model badge + chat gating; the live
    // shell model itself comes from a hook (useShellSessionModel), not a prop.
    pp.backing?.kind === np.backing?.kind &&
    backingPluginId(pp) === backingPluginId(np) &&
    backingSessionId(pp) === backingSessionId(np) &&
    pp.protected === np.protected &&
    pp.role === np.role &&
    pp.chat_view === np.chat_view &&
    pp.pending_prompt === np.pending_prompt &&
    pp.agent_session_model === np.agent_session_model &&
    pp.parent_pane_id === np.parent_pane_id &&
    pp.depth === np.depth &&
    JSON.stringify(pp.connected_services) === JSON.stringify(np.connected_services)
  );
}

export const TabItem = memo(TabItemInner, tabItemPropsEqual);
