import { memo, useEffect, useMemo, useRef, useState } from "react";
import { ShieldCheck } from "lucide-react";
import { usePluginTabMenu } from "../../lib/pluginTabMenu";
import { useTabProvider, useShellSessionModel } from "../../lib/useTabProvider";
import { getAgentDescriptor, listAgentDescriptors } from "../../lib/agentDescriptorCache";
import { TabMenu } from "../Terminal/TabMenu";
import { buildTabMenu, type TabMenuAction } from "../Terminal/tabMenuModel";
import { performTabMenuAction } from "../Terminal/tabMenuActions";
import { useTelegramThreadMenu } from "../Terminal/useTelegramThreadMenu";
import { ActivityDots } from "../shared/ActivityDots";
import { ChoiceIcon, TelegramIcon, TerminalIcon } from "../Icons";
import { ProviderGlyph } from "../ProviderGlyph";
import type { Tab, TabStatus } from "../../lib/types";
import { anchorsTeam, isGeneralAgent as isGeneralAgentTab } from "../../lib/roleKind";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { formatModelBadge } from "../../lib/formatModelBadge";
import { agentBadgeLabel, agentModelForBadge, classifyAgentLiveness } from "../../lib/agentLabel";
import { SidebarModelBadge } from "./SidebarModelBadge";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { RoleBadge } from "./RoleBadge";
import { managedByLabel, remoteIdentity } from "./remoteOwnership";
import { roleShieldLabel } from "./orchestrationSections";
import { ActiveTabPill } from "./ActiveTabPill";
import type { TabTabActivity } from "../Canvas/tabTabActivity";
import {
  type ActiveWorkSnapshot,
} from "../Canvas/planView/activeWork";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { StatusDot } from "./StatusDot";
import {
  declaredAttentionLabel,
  type DeclaredAttentionLevel,
} from "./declaredAttention";

interface TabItemProps {
  tab: Tab;
  index: number;
  isActive: boolean;
  onClick: () => void;
  onClose: () => void;
  canClose: boolean;
  workspaceId: string;
  hasAgent?: boolean;
  status?: TabStatus | null;
  declaredAttention?: DeclaredAttentionLevel | null;
  onRename?: (title: string) => void;
  orchestrators?: Tab[];
  isAppearing?: boolean;
  isClosing?: boolean;
  touchMode?: boolean;
  uiFontScale?: number;
  /** Plan-membership glance (resolved from the cheap index gate by the Sidebar). */
  activity?: TabTabActivity | null;
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

function TabSubline({ subtitle, generalAgent }: { subtitle: string | null; generalAgent: boolean }) {
  if (!subtitle) return null;
  return (
    <span
      data-testid="tab-subline"
      className="flex min-w-0 items-center gap-1 truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]"
    >
      {generalAgent && !subtitle.includes(" · @") ? (
        <span data-testid="general-agent-subtitle" className="shrink-0">General Agent</span>
      ) : (
        <RoleBadge label={subtitle} className="shrink-0" />
      )}
    </span>
  );
}

/** What the tab is currently busy with — subordinate to the name, above the role. */
function TabDescriptionSubtitle({ description }: { description: string | null }) {
  const text = description?.trim() ?? "";
  if (!text) return null;
  return (
    <span
      data-testid="tab-description-subtitle"
      className="block truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]/80"
    >
      {text}
    </span>
  );
}

function RemoteIdentityLine({ tab }: { tab: Tab }) {
  const identity = remoteIdentity(tab);
  if (!identity) return null;
  return (
    <span
      data-testid="remote-tab-identity"
      className="flex min-w-0 items-center gap-1 truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]"
    >
      {identity.role ? <RoleBadge label={identity.role} className="shrink-0" /> : null}
      {identity.role ? <span aria-hidden="true">·</span> : null}
      <span className="truncate">{identity.peer}</span>
    </span>
  );
}

function ManagedByLine({ label }: { label: string | null }) {
  if (!label) return null;
  return (
    <span
      data-testid="remote-managed-by"
      className="block truncate text-[10px] font-mono font-semibold uppercase leading-3 text-[var(--ct-accent)]"
    >
      {label}
    </span>
  );
}

function TabItemInner({
  isActive,
  onClick,
  onClose,
  canClose,
  workspaceId: _workspaceId,
  hasAgent,
  status,
  declaredAttention,
  onRename: _onRename,
  tab,
  orchestrators,
  isAppearing,
  isClosing,
  touchMode = false,
  uiFontScale,
  activity,
  activeWork: _activeWork = null,
  reportCount = 0,
  groupCollapsible = false,
  collapsed = false,
  onToggleCollapse,
  collapsedSummary,
}: TabItemProps) {
  // Unified identity for agent (agent_type) AND chatBackend shell (backing.plugin_id)
  // tabs — drives the icon, tooltip, and (for shells) the model badge.
  const provider = useTabProvider(tab);
  // useTabProvider already subscribes to the descriptor cache, so this read
  // re-resolves with it — the menu needs the models the glyph does not.
  const descriptor = getAgentDescriptor(tab.agent_type);
  const shellSessionModel = useShellSessionModel(tab);
  const providerName = provider?.displayName || "Agent";
  // Runtime session evidence wins; own-tab launch provenance covers startup gaps.
  const modelEvidence = {
    sessionModel: tab.agent_session_model,
    providerResolvedModel: tab.agent_provider_resolved_model,
    requestedModel: tab.agent_requested_model,
  };
  const tabModel = agentModelForBadge(modelEvidence) ?? shellSessionModel;
  const agentLiveness = classifyAgentLiveness({
    declaresAgent: tab.agent_type != null,
    ptyLive: tab.pty_id && status != null ? status !== "Dead" : null,
    chatSessionOpen: tab.backing?.kind === "chat_backend" ? tab.agent_session_id != null : null,
    bootStage: tab.boot_stage ?? null,
    bootInFlight: tab.agent_booting ?? false,
  });
  // Compose from current tab evidence so a cached provider-only label cannot
  // mask a newer runtime model.
  const tabAgentLabel = agentBadgeLabel(tab.agent_type, modelEvidence, agentLiveness);
  // Only a chatBackend shell's own detected model stands alone; a PTY tab that
  // lost its provider must not keep the model half of the identity.
  const shellBadge = shellSessionModel ? formatModelBadge(shellSessionModel) : null;
  const tabModelBadge = tabAgentLabel ?? shellBadge;
  const description = tab.description ?? null;
  const canonicalTitle = displayTitleForTab(tab);
  const displayTitle = hasAgent ? canonicalTitle.replace(/^[✻✳✴✵✶✷✸✹✺✻✼✽✾✿❀❁❂❃❇❈❉❊❋*]\s*/u, "") : canonicalTitle;
  const isGeneralAgent = isGeneralAgentTab(tab);
  const accessibleTitle = isGeneralAgent ? "General Agent" : displayTitle;
  const roleShield = isGeneralAgent ? "General Agent" : roleShieldLabel(tab);
  const managedBy = managedByLabel(tab);
  const localSubtitle = tab.remote_peer_id ? null : roleShield;
  const isSubTab =
    !anchorsTeam(tab) && (Boolean(tab.parent_tab_id) || (tab.depth ?? 0) > 0);
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

  const pluginTabMenu = usePluginTabMenu();
  const telegramThread = useTelegramThreadMenu(tab.id, !tab.remote_peer_id);
  const menuEntries = useMemo(() => buildTabMenu({
    tab,
    surface: "sidebar",
    status: status ?? null,
    descriptor,
    descriptors: listAgentDescriptors(),
    activity: activity ?? null,
    orchestrators,
    canRename: true,
    canClose,
    pluginTabMenu,
    telegramThread: telegramThread.state,
  }), [activity, canClose, descriptor, orchestrators, pluginTabMenu, status, tab, telegramThread.state]);

  const runMenuAction = (action: TabMenuAction) => {
    performTabMenuAction(tab.id, action, {
      rename: () => { setEditName(canonicalTitle); setEditing(true); },
      close: onClose,
    }).catch((error) => console.warn("Tab action failed:", error));
  };

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
      role="group"
      // Name the row from the title alone — the tooltip also carries the live
      // description, which would make the accessible name churn on every update.
      aria-label={accessibleTitle}
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
        aria-label={`${isGeneralAgent ? accessibleTitle : `Open tab ${accessibleTitle}`}${managedBy ? `, ${managedBy}` : ""}`}
        aria-current={isActive ? "page" : undefined}
        onClick={onClick}
        className={`sidebar-tab-activation flex flex-1 items-center min-w-0 relative z-[1] border-0 bg-transparent p-0 text-left text-inherit font-inherit cursor-pointer ${touchMode ? "gap-2.5" : "gap-2"}`}
      >
      <div className={`flex flex-1 items-center min-w-0 ${touchMode ? "gap-2.5" : "gap-2"}`}>
        <span data-testid="tab-tab-icon" className="shrink-0 leading-none">
          {provider ? (
            <ProviderGlyph
              iconSvg={provider.iconSvg}
              iconColor={provider.iconColor}
              iconHint={provider.iconHint}
              className={`leading-none ${touchMode ? "text-[15px]" : isSubTab ? "text-[10px]" : "text-[12px]"}`}
              title={status === "Working" ? `${providerName} working...` : status === "Waiting" ? `${providerName} — waiting for input` : providerName}
            />
          ) : tab.tab_type === "Choice" ? (
            <ChoiceIcon className={`${touchMode ? "scale-110" : isSubTab ? "scale-90" : ""}`} />
          ) : (
            <TerminalIcon className={`${touchMode ? "scale-110" : isSubTab ? "scale-90" : ""}`} />
          )}
        </span>
        <span className="min-w-0 flex-1">
            <span className="flex min-w-0 items-center gap-1.5">
              <span
                data-testid="tab-title"
                className={`${touchMode ? tabTouchText : tabText} block truncate min-w-0 text-[var(--ct-fg)] ${isActive ? "font-medium" : ""}`}
                onDoubleClick={(e) => {
                  if (tab?.protected) return; // Main is unrenameable
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
              <>
                <TabDescriptionSubtitle description={description} />
                <TabSubline subtitle={localSubtitle} generalAgent={isGeneralAgent} />
                <RemoteIdentityLine tab={tab} />
                <ManagedByLine label={managedBy} />
              </>
            )}
        </span>
        {/* Connected services */}
        {tab?.connected_services?.includes("telegram") && (
          <span
            role="img"
            aria-label={`${accessibleTitle} is connected to Telegram`}
            className="shrink-0 text-[var(--ct-muted-fg)]/50"
          >
            <TelegramIcon />
          </span>
        )}
        {tab?.permission_auto_confirm && (
          <span
            role="img"
            aria-label={`${accessibleTitle} auto-confirms recognized permission prompts`}
            title="Auto-confirms recognized permission prompts"
            data-testid="tab-auto-confirm-indicator"
            className="shrink-0"
            style={{ color: "var(--ct-green)" }}
          >
            <ShieldCheck size={12} aria-hidden />
          </span>
        )}
        {/* Prompt/status indicator dot */}
        {tab?.pending_prompt && (
          <span
            role="img"
            className="shrink-0 w-2 h-2 rounded-full"
            style={{ background: "var(--ct-yellow)" }}
            title={tab.pending_prompt.question}
          />
        )}
        {declaredAttention && (
          <StatusDot
            status="Waiting"
            label={declaredAttentionLabel(declaredAttention)}
          />
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
        aria-label={`${accessibleTitle} tab status ${status ?? "Unknown"}`}
      >
        {accessibleTitle} tab status {status ?? "Unknown"}
      </span>
      <div className="shrink-0 relative z-[1] flex items-center min-w-5 justify-end">
        {tabModelBadge && !touchMode ? (
          <SidebarModelBadge label={tabModelBadge} title={tabModel ?? undefined} />
        ) : null}
        {menuEntries.length > 0 && (
          <TabMenu
            entries={menuEntries}
            onAction={runMenuAction}
            onOpenChange={(open) => {
              if (open) telegramThread.refresh();
            }}
            contextMenuTargetRef={tabRef}
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

function backingPluginId(p: Tab): string | undefined {
  return p.backing?.kind === "chat_backend" ? p.backing.plugin_id : undefined;
}
function backingSessionId(p: Tab): string | undefined {
  return p.backing?.kind === "chat_backend" ? p.backing.session_id : undefined;
}

function tabItemPropsEqual(prev: TabItemProps, next: TabItemProps): boolean {
  if (
    prev.isActive !== next.isActive ||
    prev.status !== next.status ||
    prev.declaredAttention !== next.declaredAttention ||
    prev.canClose !== next.canClose ||
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

  const pp = prev.tab;
  const np = next.tab;
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
    // The row menu's model entry needs a live PTY to write the switch into.
    pp.pty_id === np.pty_id &&
    // shell model itself comes from a hook (useShellSessionModel), not a prop.
    pp.backing?.kind === np.backing?.kind &&
    backingPluginId(pp) === backingPluginId(np) &&
    backingSessionId(pp) === backingSessionId(np) &&
    pp.protected === np.protected &&
    pp.role_profile === np.role_profile &&
    pp.role === np.role &&
    pp.role_profile === np.role_profile &&
    JSON.stringify(pp.remote_channels) === JSON.stringify(np.remote_channels) &&
    pp.ownership?.controller_title === np.ownership?.controller_title &&
    pp.ownership?.team?.id === np.ownership?.team?.id &&
    pp.remote_peer_name === np.remote_peer_name &&
    pp.remote_peer_id === np.remote_peer_id &&
    pp.chat_view === np.chat_view &&
    pp.permission_auto_confirm === np.permission_auto_confirm &&
    pp.pending_prompt === np.pending_prompt &&
    pp.agent_session_model === np.agent_session_model &&
    pp.agent_provider_resolved_model === np.agent_provider_resolved_model &&
    pp.agent_requested_model === np.agent_requested_model &&
    pp.boot_stage === np.boot_stage &&
    pp.agent_booting === np.agent_booting &&
    pp.pty_id === np.pty_id &&
    pp.agent_state?.status_line === np.agent_state?.status_line &&
    pp.agent_state?.state === np.agent_state?.state &&
    pp.parent_tab_id === np.parent_tab_id &&
    pp.depth === np.depth &&
    JSON.stringify(pp.connected_services) === JSON.stringify(np.connected_services)
  );
}

export const TabItem = memo(TabItemInner, tabItemPropsEqual);
