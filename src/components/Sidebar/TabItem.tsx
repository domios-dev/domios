import { idleBackgroundLabel, tabActivity, type TabActivity } from "../../lib/tabActivity";
import { isFirstRunGeneralTab } from "../AgentPicker/constants";
import { ShellPurposeIcon } from "../shared/ShellStatus";
import { isUtilityShell, shellPurposeLabel } from "../../lib/shellCommandState";
import { createContext, memo, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Globe, SquareTerminal, TriangleAlert } from "lucide-react";
import { surfaceBacking, tabRowLabel } from "../../lib/surfaceTab";
import { SurfaceIcon } from "../Surface/surfaceIcon";
import { usePluginTabMenu } from "../../lib/pluginTabMenu";
import { useTabProvider, useShellSessionModel } from "../../lib/useTabProvider";
import { getAgentDescriptor, listAgentDescriptors } from "../../lib/agentDescriptorCache";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { useTabAccounts } from "../../hooks/useAccountSelectors";
import { AccountAvatar } from "../shared/AccountAvatar";
import { accountIdentity, tabAccountIndicator } from "../shared/accountChoices";
import { TabMenu } from "../Terminal/TabMenu";
import { useLiveTranscriptHost } from "../Terminal/useLiveTranscriptPanel";
import { buildTabMenu, unavailableTabMenu, type TabMenuAction } from "../Terminal/tabMenuModel";
import { performTabMenuAction } from "../Terminal/tabMenuActions";
import { useProviderSwitchDialog } from "./ProviderSwitchDialog";
import { useTelegramThreadMenu } from "../Terminal/useTelegramThreadMenu";
import { ActivityDots } from "../shared/ActivityDots";
import { RowMenuButton } from "../shared/RowMenuButton";
import { ChoiceIcon, TelegramIcon, TerminalIcon } from "../Icons";
import { ProviderGlyph } from "../ProviderGlyph";
import type { Tab, TabStatus } from "../../lib/types";
import { anchorsTeam, isGeneralAgent as isGeneralAgentTab } from "../../lib/roleKind";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { agentModelForBadge } from "../../lib/agentLabel";
import { plainModelId } from "../../lib/agentModelName";
import { restoreFailurePresentation } from "../../lib/restoreFailurePresentation";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { RoleBadge } from "./RoleBadge";
import { managedByLabel } from "./remoteOwnership";
import { RemoteMachineSummary, RemoteTabIcon, RemoteTabMachineGlyph, useRemoteTabPresentation, type ObservedTabMachine } from "./RemoteTabPresentation";
import { matchesSidebarMachine, SidebarWholeTeam, SidebarWholeWorkspace, useSidebarMachineFilter } from "./sidebarMachineFilter";
import { managedTabPresentation } from "../../lib/managedTab";
import { roleShieldLabel } from "./orchestrationSections";
import { ActiveTabPill } from "./ActiveTabPill";
import { useSidebarSelectedRow } from "./sidebarSelectedRow";
import type { TabTabActivity } from "../Canvas/tabTabActivity";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { TabAttentionMark } from "./TabAttentionMark";
import {
  tabAttention, type TabAttention,
} from "./tabAttention";
import { tabRowText } from "./tabRowModel";
import { sameRowTab } from "./tabRowEquality";
import { useLiveRowTab } from "./useLiveRowTab";
import { ChecklistToggleButton, type ChecklistToggle } from "./AgentChecklist";
import { LockBadge } from "../billing/LockBadge";
import { openLockedPlans } from "../billing/freeLimitGate";
import { useTabLock } from "../billing/useTabLock";
import { lockedDimClass } from "../billing/freeLimitModel";

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
  /** Agent row disclosure for its read-only checklist detail. */
  checklistToggle?: ChecklistToggle | null;
  startedBy?: TabStartedBy | null;
  observedMachine?: ObservedTabMachine | null;
  readOnly?: boolean;
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

function TabBackgroundActivity({ label }: { label: string }) {
  return (
    <span
      data-testid="tab-background-activity"
      role="img"
      aria-label={label}
      title={label}
      className="inline-flex items-center shrink-0 text-[var(--ct-muted-fg)]"
    >
      <SquareTerminal size={12} aria-hidden="true" />
    </span>
  );
}

export const GeneralListMachineContext = createContext<string | null>(null);

function TabSubline({ role, generalAgent, machine }: { role: string | null; generalAgent: boolean; machine?: string | null }) {
  if (!role) return null;
  return (
    <span data-testid="tab-subline" className="flex min-w-0 items-center gap-1 truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]">
      {generalAgent ? <span className="shrink-0" data-testid="general-agent-subtitle">{role}</span> : <span><RoleBadge label={role} /></span>}
      {machine && <>
        <span className="shrink-0" aria-hidden="true"> · </span>
        <span className="min-w-0 truncate" title={machine}>{machine}</span>
      </>}
    </span>
  );
}

function TabDescriptionSubtitle({ description }: { description: string | null }) {
  const text = description?.trim() ?? "";
  if (!text) return null;
  return (
    <span
      data-testid="tab-description-subtitle"
      className="block truncate text-[10px] leading-3 text-[var(--ct-muted-fg)] opacity-70"
    >
      {text}
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
  tab: rowTab,
  orchestrators,
  isAppearing,
  isClosing,
  touchMode = false,
  uiFontScale,
  activity,
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
  checklistToggle = null,
  observedMachine = null,
  readOnly = false,
  trailingSlot,
  accounts,
}: TabItemProps & { trailingSlot?: ReactNode; accounts?: ReturnType<typeof useTabAccounts> }) {
  const tab = useLiveRowTab(rowTab);
  const transcript = useLiveTranscriptHost();
  const liveInput = transcript?.locked && transcript.ownerId === tab.id;
  const machineFilter = useSidebarMachineFilter();
  const wholeTeam = useContext(SidebarWholeTeam);
  const wholeWorkspace = useContext(SidebarWholeWorkspace);
  const localGeneralMachine = useContext(GeneralListMachineContext);
  const lock = useTabLock(tab);
  const activate = () => {
    if (!observedMachine?.cached && !openLockedPlans(lock)) onClick();
  };
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
  const menuModel = restoreFailure.showIdentityBadges && tabModel ? plainModelId(tabModel) : null;
  const menuSubtitle = utilityShell
    ? shellPurposeLabel(tab.purpose)
    : provider
      ? [providerName, menuModel].filter(Boolean).join(" · ")
      : menuModel ?? "Terminal";
  const menuIcon = provider ? (
    <ProviderGlyph iconSvg={provider.iconSvg} fallbackIconSvg={provider.fallbackIconSvg} iconColor={provider.iconColor} iconHint={provider.iconHint} className="leading-none text-[16px]" />
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
  const remote = useRemoteTabPresentation(tab, tabState, observedMachine);
  const remoteStateLabel = remote?.unavailable;
  const machine = remote?.name;
  const generalMachine = localGeneralMachine && isGeneralAgent && !wholeTeam ? machine ?? localGeneralMachine : null;
  const remoteLabel = remote ? `${displayTitle}, ${remote.label}` : null;
  const accessibleRowLabel = remoteLabel ?? accessibleTitle;
  const showActivityMarkers = !remoteStateLabel;
  const rowAttention = attention ?? tabAttention(tab, status, [], isActive);
  const backgroundLabel = hasAgent && showActivityMarkers ? idleBackgroundLabel(tabState) : null;
  const backgroundActive = Boolean(backgroundLabel);
  // A Team worker reads as name and role label; its description stays in the row's tooltip.
  const labelledWorker = tab.role === "Worker" && Boolean(tab.role_label?.trim());
  const activityDescription = hasAgent && tabState.phase === "compacting"
    ? tabState.label
    : backgroundLabel
      ? `Idle · ${backgroundLabel}`
      : labelledWorker ? null : description;
  const rowText = tabRowText({ title: displayTitle, description: activityDescription, typeLabel: generalMachine ? accessibleTitle : roleShield });
  const isSubTab =
    !anchorsTeam(tab) && (Boolean(tab.parent_tab_id) || (tab.depth ?? 0) > 0);
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(canonicalTitle);
  const tabRef = useRef<HTMLDivElement>(null);
  const selectedRowRef = useSidebarSelectedRow(tab.id, rowText.title, isActive, tabRef);
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

  const { openProviderSwitch, providerSwitchDialog } = useProviderSwitchDialog(tab);
  const pluginTabMenu = usePluginTabMenu();
  const telegramThread = useTelegramThreadMenu(tab.id, !readOnly && !tab.remote_peer_id);
  const observedPeerId = observedMachine?.peerId;
  const observedName = remote?.name;
  const standardMenu = useMemo(() => buildTabMenu({
    tab: readOnly && observedPeerId && !tab.remote_peer_id
      ? { ...tab, remote_peer_id: observedPeerId, remote_peer_name: observedName }
      : tab,
    surface: "sidebar",
    lock,
    status: tabState.status,
    descriptor,
    descriptors: listAgentDescriptors(),
    activity: activity ?? null,
    orchestrators,
    canRename: true,
    canClose,
    pluginTabMenu,
    telegramThread: telegramThread.state,
  }), [activity, canClose, descriptor, lock, observedName, observedPeerId, orchestrators, pluginTabMenu, readOnly, tabState.status, tab, telegramThread.state]);

  const menuEntries = readOnly
    ? unavailableTabMenu(standardMenu, remoteStateLabel ?? "Open this tab to use its actions.")
    : standardMenu;

  const runMenuAction = (action: TabMenuAction) => {
    performTabMenuAction(tab.id, action, {
      sessionBindingNonce: tab.session_binding_nonce,
      switchProvider: openProviderSwitch,
      rename: () => { setEditName(canonicalTitle); setEditing(true); },
      close: onClose,
    }).catch((error) => console.warn("Tab action failed:", error));
  };

  const tabMenu = menuEntries.length > 0 || remote ? (
    <TabMenu
      tab={readOnly ? undefined : tab}
      accounts={accounts}
      entries={menuEntries}
      onAction={runMenuAction}
      onOpenChange={(open) => {
        if (open && !readOnly) telegramThread.refresh();
      }}
      contextMenuTargetRef={remote && touchMode ? undefined : tabRef}
      className={touchMode ? remote ? "shrink-0" : "sr-only" : "sidebar-tab-kebab"}
      identity={remote ? undefined : { icon: menuIcon, title: isGeneralAgent ? "General Agent" : displayTitle, subtitle: menuSubtitle }}
      provenance={remote && <RemoteMachineSummary machine={remote} />}
      trigger={<RowMenuButton label="Tab actions" touchMode={touchMode} />}
    />
  ) : null;

  const iconStatusLabel = tabState.label || backgroundLabel;
  const harnessIcon = provider ? (
            <ProviderGlyph
              iconSvg={provider.iconSvg} fallbackIconSvg={provider.fallbackIconSvg}
              iconColor={provider.iconColor}
              iconHint={provider.iconHint}
              className={`leading-none ${touchMode ? "text-[15px]" : isSubTab ? "text-[10px]" : "text-[12px]"}`}
              title={remote ? undefined : iconStatusLabel ? `${providerName} · ${iconStatusLabel}` : providerName}
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
          );

  const animClass = isClosing
    ? closePhase === 2
      ? "sidebar-tab-closing-2"
      : "sidebar-tab-closing-1"
    : isAppearing
      ? "sidebar-tab-appear"
      : "";

  return (
    <div
      ref={selectedRowRef}
      hidden={!wholeWorkspace && !wholeTeam && !matchesSidebarMachine(machineFilter, remote?.peerId)}
      role="group"
      aria-label={accessibleRowLabel}
      data-testid="tab-item"
      data-tab-id={tab.id}
      data-tab-activity={tabState.kind}
      data-background-work={backgroundActive ? "true" : undefined}
      data-entering={isAppearing && !isClosing ? "true" : undefined}
      data-tab-no-task-bubble="true"
      title={[displayTitle, provider?.displayName, description].filter(Boolean).join("\n")}
      data-variant={variant}
      style={insetPx != null ? { paddingLeft: insetPx } : undefined}
      className={`
        sidebar-tab group flex items-center justify-between cursor-pointer transition-colors relative
        ${touchMode ? "sidebar-tab-touch" : ""}
        ${attachment ? "sidebar-tab--attachment" : ""}
        ${touchMode ? (attachment ? "min-h-9 pl-5 pr-2 py-1" : "min-h-11 pl-5 pr-2 py-2") : attachment ? "pl-4 pr-4 py-0.5" : "pl-4 pr-4 py-1.5"}
        ${animClass}
        ${isActive || editing ? "text-[var(--ct-fg)]" : "sidebar-tab-dimmed"}
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
              if (e.key !== "Enter" && e.key !== "Escape") return;
              e.preventDefault();
              e.stopPropagation();
              if (e.key === "Enter") {
                (e.target as HTMLInputElement).blur();
              } else {
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
        data-first-run-allowed={isFirstRunGeneralTab(tab) || undefined}
        data-first-run-home={isFirstRunGeneralTab(tab) || undefined}
        aria-disabled={observedMachine?.cached || (isFirstRunGeneralTab(tab) ? false : undefined)}
        aria-label={`${remoteLabel ?? (isGeneralAgent ? accessibleTitle : `Open tab ${accessibleTitle}`)}${managedBy ? `, ${managedBy}` : ""}${restoreFailure.showFailureBadge ? ", launch failed" : ""}`}
        aria-current={isActive ? "page" : undefined}
        onClick={activate}
        className={`sidebar-tab-activation flex flex-1 items-center min-w-0 relative z-[1] border-0 bg-transparent p-0 text-left text-inherit font-inherit cursor-pointer ${touchMode ? "gap-2.5" : "gap-2"}`}
      >
      <div className={`sidebar-tab-body flex-1 min-w-0 ${touchMode ? "gap-x-2.5" : "gap-x-2"}`}>
        <span data-testid="tab-tab-icon" title={remote ? "" : undefined} className={`sidebar-tab-body__icon relative leading-none ${lockedDimClass(lock)}`}>
          {remote ? <RemoteTabIcon machine={remote}>
            {harnessIcon}
            <TabAttentionMark title={accessibleTitle} attention={rowAttention} active={isActive} remote />
          </RemoteTabIcon> : <>
            {harnessIcon}
            <TabAttentionMark title={accessibleTitle} attention={rowAttention} active={isActive} />
          </>}
        </span>
        <span className="sidebar-tab-body__title flex min-w-0 items-center gap-1.5">
          <span
            data-testid="tab-title"
            className={`${touchMode && !attachment ? tabTouchText : tabText} block truncate min-w-0 ${remoteStateLabel ? "text-[var(--ct-fg)]/60" : attachment && !isActive ? "text-[var(--ct-muted-fg)]" : "text-[var(--ct-fg)]"} ${isActive ? "font-semibold" : ""} ${lockedDimClass(lock)}`}
            onDoubleClick={(e) => {
              if (readOnly || tab?.protected) return; // Main is unrenameable
              e.stopPropagation();
              setEditName(canonicalTitle);
              setEditing(true);
            }}
          >
            {rowText.title}
          </span>
          {liveInput && <span className="ct-live-sidebar-mark shrink-0" title="Live Input owner" aria-label="Live Input owner">●</span>}
          {showActivityMarkers && <TabAttentionMark title={accessibleTitle} attention={rowAttention} active={isActive} placement="title" remote={Boolean(remote)} />}
          {showActivityMarkers && hasAgent && (
            tabState.active
              ? <TabWorkingActivity activity={tabState} label={tabState.label} />
              : backgroundLabel && <TabBackgroundActivity label={backgroundLabel} />
          )}
          {showActivityMarkers && shellRunning && <TabWorkingActivity activity={tabState} label={`${displayTitle} working`} />}
        </span>
        <span className={`sidebar-tab-body__trailing ${touchMode ? "gap-2.5" : "gap-2"}`}>
          <LockBadge view={lock} />
          {tab?.connected_services?.includes("telegram") && (
            <span
              role="img"
              aria-label={`${accessibleTitle} is connected to Telegram`}
              className="shrink-0 text-[var(--ct-muted-fg)]/50"
            >
              <TelegramIcon />
            </span>
          )}
          {reportCount > 0 && (
            <span className="inline-flex shrink-0 items-center justify-center min-w-[14px] h-3.5 px-0.5 rounded-full bg-[var(--ct-accent)]/20 text-[var(--ct-accent)] text-[9px] font-bold leading-none">
              {reportCount}
            </span>
          )}
          {trailingSlot}
        </span>
        {(!attachment || remote) && (
          <span className="sidebar-tab-body__sub">
            {remote && <RemoteTabMachineGlyph machine={remote} />}
            {collapsed && groupCollapsible && collapsedSummary ? (
              <span data-testid="group-collapsed-summary" className="truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]">
                {collapsedSummary}
              </span>
            ) : !attachment && (rowText.typeLabel || rowText.description?.trim()) && (
              <span className="min-w-0">
                <TabSubline role={rowText.typeLabel} generalAgent={isGeneralAgent} machine={generalMachine} />
                <TabDescriptionSubtitle description={rowText.description} />
              </span>
            )}
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
      {showActivityMarkers && !utilityShell && <span
        className="sr-only"
        role="status"
        aria-live="polite"
        aria-atomic="true"
        aria-label={`${accessibleTitle} tab status ${tabState.phase === "compacting" ? tabState.label : tabState.status}${backgroundLabel ? `, ${backgroundLabel}` : ""}`}
      >
        {accessibleTitle} tab status {tabState.phase === "compacting" ? tabState.label : tabState.status}{backgroundLabel && `, ${backgroundLabel}`}
      </span>}
      <div className={`shrink-0 relative z-[1] flex items-center justify-end ${touchMode ? "" : `sidebar-tab-end ${tabText}`}`}>
        {checklistToggle && <ChecklistToggleButton toggle={checklistToggle} title={displayTitle} />}
        {attachmentToggle && attachmentToggle.count > 0 && <AttachmentToggleButton toggle={attachmentToggle} />}
        {touchMode && tabMenu}
      </div>
      {!touchMode && tabMenu}
      {providerSwitchDialog}
    </div>
  );
}

function backingPluginId(p: Tab | null | undefined): string | undefined {
  return p?.backing?.kind === "chat_backend" ? p.backing.plugin_id : undefined;
}

/** What a row shows; its click, close and rename handlers stay out because their identity never changes the row. */
export type TabItemState = Omit<TabItemProps, "onClick" | "onClose" | "onRename">;

export function tabItemPropsEqual(prev: TabItemState, next: TabItemState): boolean {
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
    prev.observedMachine?.peerId !== next.observedMachine?.peerId ||
    prev.observedMachine?.name !== next.observedMachine?.name ||
    prev.observedMachine?.availability !== next.observedMachine?.availability ||
    prev.observedMachine?.cached !== next.observedMachine?.cached ||
    prev.observedMachine?.teamName !== next.observedMachine?.teamName ||
    prev.readOnly !== next.readOnly ||
    prev.variant !== next.variant ||
    prev.insetPx !== next.insetPx ||
    prev.ownerBadgeTab?.id !== next.ownerBadgeTab?.id ||
    prev.ownerBadgeTab?.agent_type !== next.ownerBadgeTab?.agent_type ||
    backingPluginId(prev.ownerBadgeTab) !== backingPluginId(next.ownerBadgeTab) ||
    prev.attachmentToggle?.count !== next.attachmentToggle?.count ||
    prev.attachmentToggle?.collapsed !== next.attachmentToggle?.collapsed ||
    prev.attachmentToggle?.onToggle !== next.attachmentToggle?.onToggle ||
    prev.checklistToggle !== next.checklistToggle ||
    prev.startedBy?.id !== next.startedBy?.id ||
    prev.startedBy?.label !== next.startedBy?.label ||
    prev.onToggleCollapse !== next.onToggleCollapse ||
    prev.reportCount !== next.reportCount
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

  return sameRowTab(prev.tab, next.tab);
}

function AccountTabItem({ providerName, ...props }: TabItemProps & { providerName: string }) {
  const accounts = useTabAccounts(props.tab);
  const account = tabAccountIndicator(accounts.snapshot, accounts.currentAccountId);
  return <TabItemInner {...props} accounts={accounts} trailingSlot={account && (
    <AccountAvatar account={account} size={14} other label={`Account: ${accountIdentity(account, providerName)}`} />
  )} />;
}

export const TabItem = memo(function TabItem(props: TabItemProps) {
  const descriptor = useAgentDescriptor(props.tab.agent_type);
  return descriptor?.accounts && !props.tab.remote_peer_id
    ? <AccountTabItem {...props} providerName={descriptor.display_name} />
    : <TabItemInner {...props} />;
}, tabItemPropsEqual);
