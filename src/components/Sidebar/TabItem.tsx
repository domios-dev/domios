import { useOptionalUpdatePanel } from "../../hooks/useUpdatePanel";
import { tabActivity, type TabActivity } from "../../lib/tabActivity";
import { isFirstRunGeneralTab } from "../AgentPicker/constants";
import { ShellPurposeIcon } from "../shared/ShellStatus";
import { isUtilityShell, shellPurposeLabel } from "../../lib/shellCommandState";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import { Globe, Network, ShieldCheck, TriangleAlert } from "lucide-react";
import { surfaceBacking, tabRowLabel } from "../../lib/surfaceTab";
import { SurfaceIcon } from "../Surface/surfaceIcon";
import { usePluginTabMenu } from "../../lib/pluginTabMenu";
import { useTabProvider, useShellSessionModel } from "../../lib/useTabProvider";
import { getAgentDescriptor, listAgentDescriptors } from "../../lib/agentDescriptorCache";
import { TabMenu } from "../Terminal/TabMenu";
import { buildTabMenu, type TabMenuAction } from "../Terminal/tabMenuModel";
import { performTabMenuAction } from "../Terminal/tabMenuActions";
import { useTelegramThreadMenu } from "../Terminal/useTelegramThreadMenu";
import { ActivityDots } from "../shared/ActivityDots";
import { RowMenuButton } from "../shared/RowMenuButton";
import { ChoiceIcon, TelegramIcon, TerminalIcon } from "../Icons";
import { ProviderGlyph } from "../ProviderGlyph";
import type { Tab, TabStatus } from "../../lib/types";
import { anchorsTeam, isGeneralAgent as isGeneralAgentTab } from "../../lib/roleKind";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { agentModelForBadge } from "../../lib/agentLabel";
import { restoreFailurePresentation } from "../../lib/restoreFailurePresentation";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { RoleBadge } from "./RoleBadge";
import { managedByLabel, remoteIdentity, meshBadgeLabel } from "./remoteOwnership";
import { managedTabPresentation } from "../../lib/managedTab";
import { roleShieldLabel } from "./orchestrationSections";
import { ActiveTabPill } from "./ActiveTabPill";
import type { TabTabActivity } from "../Canvas/tabTabActivity";
import {
  type ActiveWorkSnapshot,
} from "../Canvas/planView/activeWork";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { TabAttentionMark } from "./TabAttentionMark";
import {
  tabAttention, type TabAttention,
} from "./tabAttention";

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
  attention?: TabAttention | null;
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
  /** Owner's title, composed into an owner-labelled surface's label. */
  ownerTitle?: string | null;
  /** Compact surface row tucked under its owner. */
  variant?: "attachment";
  /** Left inset in px; an attachment sits one avatar in from its owner. */
  insetPx?: number;
  /** Narrow sidebar: the owner's glyph rides on the surface icon instead of a tether. */
  ownerBadgeTab?: Tab | null;
  /** Owner row folding its surface attachments. */
  attachmentToggle?: AttachmentToggle | null;
  startedBy?: TabStartedBy | null;
}

export interface TabStartedBy {
  id: string | null;
  label: string;
  onOpen?: () => void;
}

export interface AttachmentToggle {
  count: number;
  collapsed: boolean;
  onToggle: () => void;
}

function AttachmentToggleButton({ toggle }: { toggle: AttachmentToggle }) {
  const noun = toggle.count === 1 ? "1 surface" : `${toggle.count} surfaces`;
  const label = toggle.collapsed ? `Show ${noun}` : `Hide ${noun}`;
  return (
    <button
      type="button"
      data-testid="attachment-toggle"
      data-collapsed={toggle.collapsed ? "true" : "false"}
      aria-expanded={!toggle.collapsed}
      aria-label={label}
      title={label}
      className="sidebar-attachment-toggle"
      onClick={(event) => {
        event.stopPropagation();
        toggle.onToggle();
      }}
    >
      <Globe size={10} aria-hidden="true" />
      {toggle.collapsed && toggle.count > 1 && <span data-testid="attachment-toggle-count">{toggle.count}</span>}
    </button>
  );
}

function OwnerBadgedSurfaceIcon({ tab, ownerTab, size }: { tab: Tab; ownerTab: Tab; size: number }) {
  const owner = useTabProvider(ownerTab);
  return (
    <span data-testid="tab-surface-owner-badge" className="sidebar-surface-badged-icon">
      <SurfaceIcon data-testid="tab-surface-icon" backing={surfaceBacking(tab)} size={size} />
      {owner && (
        <ProviderGlyph
          iconSvg={owner.iconSvg}
          iconColor={owner.iconColor}
          iconHint={owner.iconHint}
          className="sidebar-surface-badged-icon__owner leading-none"
          title={owner.displayName}
        />
      )}
    </span>
  );
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

function TabWorkingActivity({ activity, label }: { activity: TabActivity; label?: string }) {
  if (!activity.active) return null;
  return (
    <span
      data-testid="tab-working-activity"
      role={label ? "img" : undefined}
      aria-label={label}
      className="inline-flex items-center shrink-0"
    >
      <ActivityDots size="small" tone="muted" rhythm={activity.phase} active={activity.active} />
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

function StartedByLine({ origin }: { origin: TabStartedBy }) {
  const open = origin.onOpen;
  return (
    <span data-testid="tab-started-by" className="block truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]/80">
      started by{" "}
      {open ? (
        <span
          role="link"
          tabIndex={-1}
          className="sidebar-tab-origin-link"
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            open();
          }}
          onKeyDown={(event) => {
            if (event.key !== "Enter") return;
            event.stopPropagation();
            open();
          }}
        >
          {origin.label}
        </span>
      ) : (
        <span title={origin.id ? `Tab ${origin.id}` : undefined}>{origin.label}</span>
      )}
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

function MeshDot({ label }: { label: string | null }) {
  if (!label) return null;
  return (
    <span data-testid="remote-managed-by" className="sidebar-mesh-dot" role="img" aria-label={label} title={label}>
      <Network size={9} strokeWidth={2.25} aria-hidden="true" />
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
  attention,
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
  ownerTitle = null,
  variant,
  insetPx,
  ownerBadgeTab = null,
  attachmentToggle = null,
  startedBy = null,
}: TabItemProps) {
  const updates = useOptionalUpdatePanel();
  const pluginId = surfaceBacking(tab)?.plugin_id ?? tab.agent_type ?? backingPluginId(tab);
  const pluginUpdate = !!pluginId && (updates?.pluginHasUpdate(pluginId) ?? false);
  const attachment = variant === "attachment";
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
  const restoreFailure = restoreFailurePresentation(
    {
      bootStage: tab.boot_stage,
      agentType: tab.agent_type,
      protectedTab: tab.protected === true,
    },
    tab.boot_failure_reason,
    false,
  );
  // The first-run marker lives in the description field; it is an internal flag, not text to show.
  const description = isFirstRunGeneralTab(tab) ? "Choose your General Agent" : tab.description ?? null;
  const canonicalTitle = displayTitleForTab(tab);
  const rowTitle = tabRowLabel(tab, canonicalTitle, ownerTitle);
  const displayTitle = hasAgent ? rowTitle.replace(/^[✻✳✴✵✶✷✸✹✺✻✼✽✾✿❀❁❂❃❇❈❉❊❋*]\s*/u, "") : rowTitle;
  const isGeneralAgent = isGeneralAgentTab(tab);
  const utilityShell = isUtilityShell(tab);
  const tabState = tabActivity(tab, status);
  const shellRunning = utilityShell && tabState.active;
  const accessibleTitle = isGeneralAgent ? "General Agent" : utilityShell ? `${shellPurposeLabel(tab.purpose)}, ${displayTitle}` : displayTitle;
  const menuModel = restoreFailure.showIdentityBadges ? tabModel : null;
  const menuSubtitle = utilityShell
    ? shellPurposeLabel(tab.purpose)
    : provider
      ? [providerName, menuModel].filter(Boolean).join(" · ")
      : menuModel ?? "Terminal";
  const menuIcon = provider ? (
    <ProviderGlyph iconSvg={provider.iconSvg} iconColor={provider.iconColor} iconHint={provider.iconHint} className="leading-none text-[16px]" />
  ) : surfaceBacking(tab) ? (
    <SurfaceIcon backing={surfaceBacking(tab)} size={16} />
  ) : tab.tab_type === "Choice" ? (
    <ChoiceIcon />
  ) : utilityShell ? (
    <ShellPurposeIcon purpose={tab.purpose} icon={tab.icon} running={false} />
  ) : (
    <TerminalIcon />
  );
  const roleShield = restoreFailure.showIdentityBadges
    ? (isGeneralAgent ? "General Agent" : roleShieldLabel(tab))
    : null;
  const managedBy = managedTabPresentation(tab.managed).label ?? managedByLabel(tab);
  const meshLabel = meshBadgeLabel(tab);
  const localSubtitle = tab.remote_peer_id ? null : roleShield;
  const isSubTab =
    !anchorsTeam(tab) && (Boolean(tab.parent_tab_id) || (tab.depth ?? 0) > 0);
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(canonicalTitle);
  const tabRef = useRef<HTMLDivElement>(null);
  const tabText = attachment
    ? pickUiFontScaleValue(uiFontScale, ["text-[10px]", "text-[10.5px]", "text-xs", "text-sm"])
    : pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
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
    status: tabState.status,
    descriptor,
    descriptors: listAgentDescriptors(),
    activity: activity ?? null,
    orchestrators,
    canRename: true,
    canClose,
    pluginTabMenu,
    telegramThread: telegramThread.state,
  }), [activity, canClose, descriptor, orchestrators, pluginTabMenu, tabState.status, tab, telegramThread.state]);

  const runMenuAction = (action: TabMenuAction) => {
    performTabMenuAction(tab.id, action, {
      sessionBindingNonce: tab.session_binding_nonce,
      rename: () => { setEditName(canonicalTitle); setEditing(true); },
      close: onClose,
    }).catch((error) => console.warn("Tab action failed:", error));
  };

  const tabMenu = menuEntries.length > 0 ? (
    <TabMenu
      entries={menuEntries}
      onAction={runMenuAction}
      onOpenChange={(open) => {
        if (open) telegramThread.refresh();
      }}
      contextMenuTargetRef={tabRef}
      className={touchMode ? "flex h-11 w-11 items-center justify-center" : "sidebar-tab-kebab"}
      identity={{ icon: menuIcon, title: isGeneralAgent ? "General Agent" : displayTitle, subtitle: menuSubtitle }}
      trigger={<RowMenuButton label="Tab actions" touchMode={touchMode} />}
    />
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
      role="group"
      aria-label={accessibleTitle}
      data-testid="tab-item"
      data-tab-id={tab.id}
      data-tab-activity={tabState.kind}
      data-tab-no-task-bubble="true"
      title={description ? `${displayTitle}\n${description}` : displayTitle}
      data-variant={variant}
      style={insetPx != null ? { paddingLeft: insetPx } : undefined}
      className={`
        sidebar-tab group flex items-center justify-between cursor-pointer transition-colors relative
        ${touchMode ? "sidebar-tab-touch" : ""}
        ${attachment ? "sidebar-tab--attachment" : ""}
        ${touchMode ? (attachment ? "min-h-9 pl-5 pr-2 py-1" : "min-h-11 pl-5 pr-2 py-2") : attachment ? "pl-4 pr-4 py-0.5" : "pl-4 pr-4 py-1.5"}
        ${animClass}
        ${isActive || editing ? "text-[var(--ct-fg)]" : "sidebar-tab-dimmed hover:bg-[rgba(255,255,255,0.04)]"}
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
            className={`${tabText} ct-input ct-input--compact bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded px-1 py-0 w-full outline-none`}
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
        aria-label={`${isGeneralAgent ? accessibleTitle : `Open tab ${accessibleTitle}`}${managedBy ? `, ${managedBy}` : ""}${restoreFailure.showFailureBadge ? ", launch failed" : ""}`}
        aria-current={isActive ? "page" : undefined}
        onClick={onClick}
        className={`sidebar-tab-activation flex flex-1 items-center min-w-0 relative z-[1] border-0 bg-transparent p-0 text-left text-inherit font-inherit cursor-pointer ${touchMode ? "gap-2.5" : "gap-2"}`}
      >
      <div className={`sidebar-tab-body flex-1 min-w-0 ${touchMode ? "gap-x-2.5" : "gap-x-2"}`}>
        <span data-testid="tab-tab-icon" className="sidebar-tab-body__icon relative leading-none">
          {provider ? (
            <ProviderGlyph
              iconSvg={provider.iconSvg}
              iconColor={provider.iconColor}
              iconHint={provider.iconHint}
              className={`leading-none ${touchMode ? "text-[15px]" : isSubTab ? "text-[10px]" : "text-[12px]"}`}
              title={tabState.label ? `${providerName} · ${tabState.label}` : providerName}
            />
          ) : surfaceBacking(tab) ? (
            ownerBadgeTab ? (
              <OwnerBadgedSurfaceIcon tab={tab} ownerTab={ownerBadgeTab} size={touchMode ? 14 : 11} />
            ) : (
              <SurfaceIcon data-testid="tab-surface-icon" backing={surfaceBacking(tab)} size={touchMode ? 15 : isSubTab ? 11 : 13} />
            )
          ) : tab.tab_type === "Choice" ? (
            <ChoiceIcon className={`${touchMode ? "scale-110" : isSubTab ? "scale-90" : ""}`} />
          ) : (
            utilityShell ? <ShellPurposeIcon purpose={tab.purpose} icon={tab.icon} running={shellRunning} /> : <TerminalIcon className={`${touchMode ? "scale-110" : isSubTab ? "scale-90" : ""}`} />
          )}
          <TabAttentionMark title={accessibleTitle} attention={attention ?? tabAttention(tab, status, [], isActive)} active={isActive} />
        </span>
        <span className="sidebar-tab-body__title flex min-w-0 items-center gap-1.5">
          <span
            data-testid="tab-title"
            className={`${touchMode && !attachment ? tabTouchText : tabText} block truncate min-w-0 ${attachment && !isActive ? "text-[var(--ct-muted-fg)]" : "text-[var(--ct-fg)]"} ${isActive ? "font-medium" : ""}`}
            onDoubleClick={(e) => {
              if (tab?.protected) return; // Main is unrenameable
              e.stopPropagation();
              setEditName(canonicalTitle);
              setEditing(true);
            }}
          >
            {displayTitle}
            {pluginUpdate && <span aria-label="Plugin update available" className="ml-1 text-[10px] text-[var(--ct-accent)]">Update</span>}
          </span>
          {hasAgent && (
            <TabWorkingActivity activity={tabState} />
          )}
          {shellRunning && <TabWorkingActivity activity={tabState} label={`${displayTitle} working`} />}
        </span>
        <span className={`sidebar-tab-body__trailing ${touchMode ? "gap-2.5" : "gap-2"}`}>
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
          {reportCount > 0 && (
            <span className="inline-flex shrink-0 items-center justify-center min-w-[14px] h-3.5 px-0.5 rounded-full bg-[var(--ct-accent)]/20 text-[var(--ct-accent)] text-[9px] font-bold leading-none">
              {reportCount}
            </span>
          )}
        </span>
        {collapsed && groupCollapsible && collapsedSummary ? (
          <span
            data-testid="group-collapsed-summary"
            className="sidebar-tab-body__sub block truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]"
          >
            {collapsedSummary}
          </span>
        ) : attachment ? null : (
          <span className="sidebar-tab-body__sub">
            <TabDescriptionSubtitle description={description} />
            {startedBy && <StartedByLine origin={startedBy} />}
            <TabSubline subtitle={localSubtitle} generalAgent={isGeneralAgent} />
            <RemoteIdentityLine tab={tab} />
            {!tab.remote_peer_id && managedByLabel(tab) && <span className="block truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]">{managedByLabel(tab)}</span>}
          </span>
        )}
      </div>
      </button>}
      {restoreFailure.showFailureBadge && (
        <span
          role="img"
          aria-label={`${accessibleTitle} launch failed`}
          title="Launch failed"
          data-testid="tab-launch-failure-badge"
          className="shrink-0 text-[var(--ct-red,#f85149)]"
        >
          <TriangleAlert size={13} aria-hidden="true" />
        </span>
      )}
      {!utilityShell && <span
        className="sr-only"
        role="status"
        aria-live="polite"
        aria-atomic="true"
        aria-label={`${accessibleTitle} tab status ${tabState.status}`}
      >
        {accessibleTitle} tab status {tabState.status}
      </span>}
      <div className={`shrink-0 relative z-[1] flex items-center justify-end ${touchMode ? "" : `sidebar-tab-end ${tabText}`}`}>
        {!attachment && !touchMode && <MeshDot label={meshLabel} />}
        {attachmentToggle && attachmentToggle.count > 0 && <AttachmentToggleButton toggle={attachmentToggle} />}
        {touchMode && tabMenu}
      </div>
      {!touchMode && tabMenu}
    </div>
  );
}

function backingPluginId(p: Tab | null | undefined): string | undefined {
  return p?.backing?.kind === "chat_backend" ? p.backing.plugin_id : undefined;
}
function backingSessionId(p: Tab): string | undefined {
  return p.backing?.kind === "chat_backend" ? p.backing.session_id : undefined;
}

function tabItemPropsEqual(prev: TabItemProps, next: TabItemProps): boolean {
  if (
    prev.isActive !== next.isActive ||
    prev.status !== next.status ||
    prev.attention !== next.attention ||
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
    prev.ownerTitle !== next.ownerTitle ||
    prev.variant !== next.variant ||
    prev.insetPx !== next.insetPx ||
    prev.ownerBadgeTab?.id !== next.ownerBadgeTab?.id ||
    prev.ownerBadgeTab?.agent_type !== next.ownerBadgeTab?.agent_type ||
    backingPluginId(prev.ownerBadgeTab) !== backingPluginId(next.ownerBadgeTab) ||
    prev.attachmentToggle?.count !== next.attachmentToggle?.count ||
    prev.attachmentToggle?.collapsed !== next.attachmentToggle?.collapsed ||
    prev.attachmentToggle?.onToggle !== next.attachmentToggle?.onToggle ||
    prev.startedBy?.id !== next.startedBy?.id ||
    prev.startedBy?.label !== next.startedBy?.label ||
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
    pp.purpose === np.purpose &&
    pp.shell_command_state?.activity === np.shell_command_state?.activity &&
    pp.shell_command_state?.last_exit_code === np.shell_command_state?.last_exit_code &&
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
    pp.boot_stage === np.boot_stage &&
    pp.boot_failure_reason === np.boot_failure_reason &&
    pp.role_profile === np.role_profile &&
    pp.role === np.role &&
    pp.roleless_tracked === np.roleless_tracked &&
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
    pp.agent_state?.phase === np.agent_state?.phase &&
    pp.agent_state?.subtype === np.agent_state?.subtype &&
    pp.parent_tab_id === np.parent_tab_id &&
    pp.depth === np.depth &&
    JSON.stringify(pp.connected_services) === JSON.stringify(np.connected_services)
  );
}

export const TabItem = memo(TabItemInner, tabItemPropsEqual);
