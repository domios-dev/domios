import { memo, useEffect, useMemo, useRef, useState } from "react";
import { Bot, Check, Copy, Cpu, Pencil, RefreshCw, RotateCcw, ShieldCheck, UserCog, X } from "lucide-react";
import { pluginTabMenuEntries, runPluginTabMenuItem, usePluginTabMenu } from "../../lib/pluginTabMenu";
import { useTabProvider, useShellSessionModel } from "../../lib/useTabProvider";
import { getAgentDescriptor } from "../../lib/agentDescriptorCache";
import { switchTabModel, tabModelEntryLabel, tabModelMenuView } from "./tabModelMenu";
import { switchTabProvider, tabProviderEntryLabel, tabProviderMenuView } from "./tabProviderMenu";
import { getTabProviderApi } from "./tabProviderApi";
import { listAgentDescriptors } from "../../lib/agentDescriptorCache";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { ActivityDots } from "../shared/ActivityDots";
import { ChoiceIcon, TelegramIcon, TerminalIcon } from "../Icons";
import { ProviderGlyph } from "../ProviderGlyph";
import { PluginIcon } from "../Extensions/PluginIcon";
import type { Tab, TabStatus } from "../../lib/types";
import { anchorsTeam, isPlanner, isGeneralAgent as isGeneralAgentTab } from "../../lib/roleKind";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { formatModelBadge } from "../../lib/formatModelBadge";
import { agentBracketLabel, classifyAgentLiveness } from "../../lib/agentLabel";
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
import { canMakeOrchestrator, makeOrchestrator, makePlanner } from "../../lib/orchestratorOnboard";
import {
  copyTabSessionId,
  tabSessionActionView,
  TAB_SESSION_LABELS,
  reconnectTabSession,
  restartTabSession,
} from "../Terminal/tabSessionActions";
import { StatusDot } from "./StatusDot";
import {
  declaredAttentionLabel,
  type DeclaredAttentionLevel,
} from "./declaredAttention";
import {
  orchestratorRecoveryMenuView,
  replaceOrchestrator,
} from "./orchestratorRecoveryMenu";

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
  // back to the live chatBackend session model (fetched/cached out of band).
  const tabModel = tab.agent_session_model ?? shellSessionModel;
  const agentLiveness = classifyAgentLiveness({
    declaresAgent: tab.agent_type != null,
    ptyLive: tab.pty_id && status != null ? status !== "Dead" : null,
    chatSessionOpen: tab.backing?.kind === "chat_backend" ? tab.agent_session_id != null : null,
    bootStage: tab.boot_stage ?? null,
    bootInFlight: tab.agent_booting ?? false,
  });
  // Recompute only for a projection predating the daemon's label, from the raw id:
  // a badge-truncated id would no longer match any catalogue entry.
  const tabAgentLabel = tab.agent_label ?? agentBracketLabel(
    tab.agent_type,
    tabModel,
    agentLiveness,
  );
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

  const menuItems: DropdownMenuItem[] = useMemo(() => {
    const items: DropdownMenuItem[] = [];
    // Main (the protected GA tab) is the anchor — no rename.
    if (!tab?.protected) {
      items.push({
        label: "Rename",
        icon: <Pencil size={11} strokeWidth={2} />,
        onClick: () => { setEditName(canonicalTitle); setEditing(true); },
      });
    }
    if (tab && canMakeOrchestrator(tab)) {
      items.push({
        label: "Make Orchestrator",
        icon: <UserCog size={11} strokeWidth={2} />,
        onClick: () => { void makeOrchestrator(tab.id); },
      });
    }
    if (
      tab
      && activity?.kind === "worker"
      && !isPlanner(tab)
    ) {
      items.push({
        label: "Make Planner",
        icon: <UserCog size={11} strokeWidth={2} />,
        onClick: () => { void makePlanner(tab.id, activity.planId); },
      });
    }
    const modelMenu = tab ? tabModelMenuView(tab, descriptor, status) : null;
    if (modelMenu) {
      items.push({
        label: tabModelEntryLabel(modelMenu),
        icon: <Cpu size={11} strokeWidth={2} />,
        disabled: Boolean(modelMenu.blockedReason),
        title: modelMenu.blockedReason ?? undefined,
        "data-testid": "tab-menu-model",
        submenu: {
          label: tabModelEntryLabel(modelMenu),
          // The switch is the provider's own in-agent command on the live
          // session — say so, because the neighbouring entry does restart one.
          description: `Switches in session — ${providerName} keeps this session and its history.`,
          items: modelMenu.models.map((model) => ({
            label: model.display_name,
            selected: model.id === modelMenu.currentModel?.id,
            icon: model.id === modelMenu.currentModel?.id
              ? <Check size={11} strokeWidth={2} />
              : <span className="w-3 h-3" />,
            onClick: () => {
              void switchTabModel(modelMenu, model.id)
                .catch((error) => console.warn("Model switch failed:", error));
            },
            "data-testid": `tab-menu-model-${model.id}`,
          })),
        },
      });
    }
    const providerMenu = tab ? tabProviderMenuView(tab, listAgentDescriptors(), status) : null;
    if (providerMenu) {
      items.push({
        label: tabProviderEntryLabel(providerMenu),
        icon: <Bot size={11} strokeWidth={2} />,
        disabled: Boolean(providerMenu.blockedReason),
        title: providerMenu.blockedReason ?? undefined,
        "data-testid": "tab-menu-provider",
        submenu: {
          label: tabProviderEntryLabel(providerMenu),
          // The neighbouring model entry switches in session; this one does not.
          description: "Restarts the session — the new provider starts fresh.",
          items: providerMenu.providers.map((provider) => ({
            label: provider.display_name,
            selected: provider.id === providerMenu.currentProviderId,
            icon: provider.id === providerMenu.currentProviderId
              ? <Check size={11} strokeWidth={2} />
              : <span className="w-3 h-3" />,
            onClick: () => {
              if (provider.id === providerMenu.currentProviderId) return;
              void switchTabProvider(getTabProviderApi(), tab.id, provider.id)
                .catch((error) => console.warn("Provider switch failed:", error));
            },
            "data-testid": `tab-menu-provider-${provider.id}`,
          })),
        },
      });
    }
    const recoveryMenu = tab
      ? orchestratorRecoveryMenuView(tab, listAgentDescriptors(), orchestrators)
      : null;
    if (recoveryMenu) {
      items.push({
        label: "Replace Orchestrator",
        icon: <UserCog size={11} strokeWidth={2} />,
        "data-testid": "tab-menu-replace-orchestrator",
        submenu: {
          label: "Replace Orchestrator",
          description: "Starts a new Team owner without resuming this session.",
          items: recoveryMenu.providers.map((candidate) => ({
            label: candidate.display_name,
            icon: <Bot size={11} strokeWidth={2} />,
            onClick: () => {
              void replaceOrchestrator(recoveryMenu, candidate.id)
                .catch((error) => console.warn("Orchestrator replacement failed:", error));
            },
            "data-testid": `tab-menu-replace-orchestrator-${candidate.id}`,
          })),
        },
      });
    }
    const sessionActions = tab ? tabSessionActionView(tab) : null;
    if (tab && sessionActions?.canReconnect) {
      items.push({
        label: TAB_SESSION_LABELS.reconnect,
        icon: <RefreshCw size={11} strokeWidth={2} />,
        onClick: () => { void reconnectTabSession(tab.id).catch((error) => console.warn("Reconnect failed:", error)); },
      });
    }
    if (tab && sessionActions?.canRestart) {
      items.push({
        label: TAB_SESSION_LABELS.restart,
        icon: <RotateCcw size={11} strokeWidth={2} />,
        onClick: () => { void restartTabSession(tab.id).catch((error) => console.warn("Restart failed:", error)); },
        "data-testid": "tab-menu-restart-session",
      });
    }
    // Plugin contributions: host-owned actions on local tabs only; a peer's tab has no local binding.
    if (tab && !tab.remote_peer_id) {
      for (const entry of pluginTabMenuEntries(pluginTabMenu, tab.id)) {
        items.push({
          label: entry.label,
          icon: <PluginIcon plugin={{ icon_svg: entry.iconSvg }} size={11} />,
          checked: entry.checked,
          onClick: () => {
            void runPluginTabMenuItem(entry, tab.id)
              .catch((error) => console.warn("Plugin tab action failed:", error));
          },
          "data-testid": `tab-menu-plugin-${entry.pluginId}-${entry.itemId}`,
        });
      }
    }
    if (canClose && !tab?.protected) {
      items.push({
        label: hasAgent ? "Close Session" : "Close Tab",
        icon: <X size={11} strokeWidth={2} />,
        onClick: onClose,
        danger: true,
        "data-testid": "tab-menu-close",
      });
    }
    return items;
  }, [activity, canonicalTitle, tab, canClose, descriptor, hasAgent, onClose, orchestrators, pluginTabMenu, providerName, status]);

  const sessionActions = tab ? tabSessionActionView(tab) : null;
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
        aria-label={TAB_SESSION_LABELS.copy}
        title={TAB_SESSION_LABELS.copy}
        disabled={!sessionId}
        onClick={(event) => {
          event.stopPropagation();
          if (sessionId) void copyTabSessionId(sessionId).catch((error) => console.warn("Copy failed:", error));
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
