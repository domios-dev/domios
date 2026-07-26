import { memo, useEffect, useMemo, useRef, useState } from "react";
import { Copy, Pencil, RefreshCw, RotateCcw, UserCog, X } from "lucide-react";
import { usePaneProvider, useShellSessionModel } from "../../lib/usePaneProvider";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { ActivityDots } from "../shared/ActivityDots";
import { TelegramIcon, TerminalIcon } from "../Icons";
import { ProviderGlyph } from "../ProviderGlyph";
import type { Pane, PaneStatus, Tab } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { formatModelBadge } from "../../lib/formatModelBadge";
import { SidebarModelBadge } from "./SidebarModelBadge";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { RoleBadge } from "./RoleBadge";
import { roleShieldLabel } from "./orchestrationSections";
import { ActiveTabPill } from "./ActiveTabPill";
import type { PaneTabActivity } from "../Canvas/paneTabActivity";
import {
  type ActiveWorkSnapshot,
} from "../Canvas/planView/activeWork";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { canMakeOrchestrator, makeOrchestrator } from "../../lib/orchestratorOnboard";
import {
  copyPaneSessionId,
  paneSessionActionView,
  PANE_SESSION_LABELS,
  reconnectPaneSession,
  restartPaneSession,
} from "../Terminal/paneSessionActions";

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
  /** Bound in-flight tasks — drives worker subline activity dots. */
  activeWork?: ActiveWorkSnapshot | null;
  /** Agent report count badge (orchestration worker rows). */
  reportCount?: number;
  /** Group head with children — overlay collapse chevron in the left padding gutter. */
  groupCollapsible?: boolean;
  collapsed?: boolean;
  onToggleCollapse?: () => void;
  /** Shown on the head only while collapsed — "1 manager · 2 workers". */
  collapsedSummary?: string | null;
}

function GroupCollapseChevron({
  touchMode,
  collapsed,
  onToggleCollapse,
}: {
  touchMode: boolean;
  collapsed: boolean;
  onToggleCollapse?: () => void;
}) {
  return (
    <SidebarDisclosure
      data-testid="group-collapse-toggle"
      data-collapsed={collapsed ? "true" : "false"}
      onClick={(e) => {
        e.stopPropagation();
        onToggleCollapse?.();
      }}
      touchMode={touchMode}
      className="shrink-0"
      title={collapsed ? "Expand orchestration" : "Collapse orchestration"}
      expanded={!collapsed}
    />
  );
}

function TabWorkingActivity() {
  return (
    <span
      data-testid="tab-working-activity"
      className="inline-flex items-center shrink-0"
    >
      <ActivityDots size="small" tone="muted" />
    </span>
  );
}

function TabSubline({ roleShield }: { roleShield: string | null }) {
  if (!roleShield) return null;
  return (
    <span
      data-testid="tab-subline"
      className="flex min-w-0 items-center gap-1 truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]"
    >
      <RoleBadge label={roleShield} className="shrink-0" />
    </span>
  );
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
  activity: _activity,
  activeWork: _activeWork = null,
  reportCount = 0,
  groupCollapsible = false,
  collapsed = false,
  onToggleCollapse,
  collapsedSummary,
}: TabItemProps) {
  // Unified identity for agent (agent_type) AND chatBackend shell (backing.plugin_id)
  // panes — drives the icon, tooltip, and (for shells) the model badge.
  const provider = usePaneProvider(pane);
  const shellSessionModel = useShellSessionModel(pane);
  const providerName = provider?.displayName || "Agent";
  // back to the live chatBackend session model (fetched/cached out of band).
  const tabModel = pane?.agent_session_model ?? shellSessionModel;
  const tabModelBadge = tabModel ? formatModelBadge(tabModel) : null;
  const roleShield = roleShieldLabel(pane);
  const description = pane?.description ?? tab.description ?? null;
  const canonicalTitle = displayTitleForTab(tab, pane);
  const displayTitle = hasAgent ? canonicalTitle.replace(/^[✻✳✴✵✶✷✸✹✺✻✼✽✾✿❀❁❂❃❇❈❉❊❋*]\s*/u, "") : canonicalTitle;
  const isSubPane =
    pane?.role !== "Orchestrator" && (Boolean(pane?.parent_pane_id) || (pane?.depth ?? 0) > 0);
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(canonicalTitle);
  const tabRef = useRef<HTMLDivElement>(null);
  const tabText = pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const tabTouchText = pickUiFontScaleValue(uiFontScale, ["text-[13px]", "text-[14px]", "text-[15px]", "text-base"]);

  const [closePhase, setClosePhase] = useState<0 | 1 | 2>(0);
  useEffect(() => {
    if (isClosing && closePhase === 0) {
      setClosePhase(1); // Phase 1: visual exit
      const timer = setTimeout(() => setClosePhase(2), 150); // Phase 2: height collapse
      return () => clearTimeout(timer);
    }
  }, [isClosing, closePhase]);

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
    const sessionActions = pane ? paneSessionActionView(pane) : null;
    if (pane && sessionActions?.canReconnect) {
      items.push({
        label: PANE_SESSION_LABELS.reconnect,
        icon: <RefreshCw size={11} strokeWidth={2} />,
        onClick: () => { void reconnectPaneSession(pane.id).catch((error) => console.warn("Reconnect failed:", error)); },
      });
    }
    if (pane && sessionActions?.canRestart) {
      items.push({
        label: PANE_SESSION_LABELS.restart,
        icon: <RotateCcw size={11} strokeWidth={2} />,
        onClick: () => { void restartPaneSession(pane.id).catch((error) => console.warn("Restart failed:", error)); },
        "data-testid": "tab-menu-restart-session",
      });
    }
    if (canClose && !pane?.protected) {
      items.push({
        label: hasAgent ? "Close Session" : "Close Tab",
        icon: <X size={11} strokeWidth={2} />,
        onClick: onClose,
        danger: true,
        "data-testid": "tab-menu-close",
      });
    }
    return items;
  }, [canonicalTitle, pane, canClose, hasAgent, onClose]);

  const sessionActions = pane ? paneSessionActionView(pane) : null;
  const sessionId = sessionActions?.sessionId ?? null;
  const sessionHeader = sessionActions?.canReconnect ? (
    <div className="mx-1 mb-1 flex min-w-48 items-center gap-1 border-b border-[var(--ct-border)] px-2 pb-1.5 pt-0.5">
      <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-[var(--ct-muted-fg)]" title={sessionId ?? "Session ID is not available yet"}>
        {sessionId ?? "ID not available yet"}
      </span>
      <button
        type="button"
        role="menuitem"
        data-testid="tab-menu-copy-session-id"
        aria-label={PANE_SESSION_LABELS.copy}
        title={PANE_SESSION_LABELS.copy}
        disabled={!sessionId}
        onClick={(event) => {
          event.stopPropagation();
          if (sessionId) void copyPaneSessionId(sessionId).catch((error) => console.warn("Copy failed:", error));
        }}
        className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] disabled:cursor-not-allowed disabled:opacity-35"
      >
        <Copy size={12} />
      </button>
    </div>
  ) : null;

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
      className={`
        sidebar-tab group flex items-center justify-between cursor-pointer transition-colors relative
        ${touchMode ? "sidebar-tab-touch" : ""}
        ${touchMode ? "min-h-11 pl-5 pr-2 py-2" : "pl-4 pr-2 py-1.5"}
        ${animClass}
        ${isActive ? "text-[var(--ct-fg)]" : "sidebar-tab-dimmed hover:bg-[rgba(255,255,255,0.04)]"}
      `}
    >
      {isActive && <ActiveTabPill />}
      {editing && (
        <div className="flex flex-1 items-center min-w-0 relative z-[1]">
          <input
            autoFocus
            aria-label={`Rename tab ${canonicalTitle}`}
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
        </div>
      )}
      {groupCollapsible && (
        <GroupCollapseChevron
          touchMode={touchMode}
          collapsed={collapsed}
          onToggleCollapse={onToggleCollapse}
        />
      )}
      {!editing && <button
        type="button"
        aria-label={`Open tab ${displayTitle}`}
        aria-current={isActive ? "page" : undefined}
        onClick={onClick}
        className={`flex flex-1 items-center min-w-0 relative z-[1] border-0 bg-transparent p-0 text-left text-inherit font-inherit cursor-pointer ${touchMode ? "gap-2.5" : "gap-2"}`}
      >
      <div className={`flex flex-1 items-center min-w-0 ${touchMode ? "gap-2.5" : "gap-2"}`}>
        <span data-testid="tab-pane-icon" className="shrink-0 leading-none">
          {provider ? (
            <ProviderGlyph
              iconSvg={provider.iconSvg}
              iconColor={provider.iconColor}
              iconHint={provider.iconHint}
              className={`leading-none ${touchMode ? "text-[15px]" : isSubPane ? "text-[10px]" : "text-[12px]"}`}
              title={status === "Working" ? `${providerName} working...` : status === "Waiting" ? `${providerName} — waiting for input` : providerName}
            />
          ) : (
            <TerminalIcon className={`${touchMode ? "scale-110" : isSubPane ? "scale-90" : ""}`} />
          )}
        </span>
        <span className="min-w-0 flex-1">
            <span className="flex min-w-0 items-center gap-1.5">
              <span
                data-testid="tab-title"
                className={`${touchMode ? tabTouchText : tabText} block truncate min-w-0 text-[var(--ct-fg)] ${isActive ? "font-medium" : ""}`}
                onDoubleClick={(e) => {
                  if (pane?.protected) return; // Main is unrenameable
                  e.stopPropagation();
                  setEditName(canonicalTitle);
                  setEditing(true);
                }}
              >
                {displayTitle}
              </span>
              {status === "Working" && <TabWorkingActivity />}
            </span>
            {collapsed && groupCollapsible && collapsedSummary ? (
              <span
                data-testid="group-collapsed-summary"
                className="block truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]"
              >
                {collapsedSummary}
              </span>
            ) : (
              <TabSubline roleShield={roleShield} />
            )}
        </span>
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
        {reportCount > 0 && (
          <span className="inline-flex shrink-0 items-center justify-center min-w-[14px] h-3.5 px-0.5 rounded-full bg-[var(--ct-accent)]/20 text-[var(--ct-accent)] text-[9px] font-bold leading-none">
            {reportCount}
          </span>
        )}
      </div>
      </button>}
      <span
        className="sr-only"
        role="status"
        aria-live="polite"
        aria-atomic="true"
        aria-label={`${displayTitle} tab status ${status ?? "Unknown"}`}
      >
        {displayTitle} tab status {status ?? "Unknown"}
      </span>
      <div className="shrink-0 relative z-[1] flex items-center min-w-5 justify-end">
        {tabModelBadge && !touchMode ? (
          <SidebarModelBadge label={tabModelBadge} title={tabModel ?? undefined} />
        ) : null}
        {menuItems.length > 0 && (
          <DropdownMenu
            items={menuItems}
            header={sessionHeader}
            contextMenuTargetRef={tabRef}
            menuLabel="Tab actions"
            className={
              touchMode
                ? "flex h-11 w-11 items-center justify-center"
                : "sidebar-tab-close absolute inset-y-0 right-0 flex items-center"
            }
            trigger={
              <button
                type="button"
                aria-label="Tab actions"
                aria-haspopup="menu"
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
    prev.tab.description !== next.tab.description ||
    prev.groupCollapsible !== next.groupCollapsible ||
    prev.collapsed !== next.collapsed ||
    prev.collapsedSummary !== next.collapsedSummary ||
    prev.onToggleCollapse !== next.onToggleCollapse ||
    prev.reportCount !== next.reportCount ||
    prev.activeWork !== next.activeWork
  ) return false;

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
    // a stale label after wake/cache-reconcile.
    pp.title === np.title &&
    pp.title_level === np.title_level &&
    pp.description === np.description &&
    pp.agent_type === np.agent_type &&
    pp.agent_session_id === np.agent_session_id &&
    // shell model itself comes from a hook (useShellSessionModel), not a prop.
    pp.backing?.kind === np.backing?.kind &&
    backingPluginId(pp) === backingPluginId(np) &&
    backingSessionId(pp) === backingSessionId(np) &&
    pp.protected === np.protected &&
    pp.role === np.role &&
    pp.chat_view === np.chat_view &&
    pp.pending_prompt === np.pending_prompt &&
    pp.agent_session_model === np.agent_session_model &&
    pp.agent_state?.status_line === np.agent_state?.status_line &&
    pp.agent_state?.state === np.agent_state?.state &&
    pp.parent_pane_id === np.parent_pane_id &&
    pp.depth === np.depth &&
    JSON.stringify(pp.connected_services) === JSON.stringify(np.connected_services)
  );
}

export const TabItem = memo(TabItemInner, tabItemPropsEqual);
