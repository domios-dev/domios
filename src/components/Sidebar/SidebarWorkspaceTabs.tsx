import { tabActivity } from "../../lib/tabActivity";
import { useMemo, type ReactNode } from "react";
import { Clock3, MonitorUp, Plus, X } from "lucide-react";
import { invoke } from "../../lib/bridge";
import type {
  Tab,
  TabStatus,
  ProviderDescriptor,
  Team,
  Workspace,
} from "../../lib/types";
import { anchorsTeam, CARTOGRAPHER_KIND, roleKind } from "../../lib/roleKind";
import { selectTeamStaffingManager, type TeamMemberRole } from "../../lib/teamActions";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Plan } from "../Canvas/types";
import type { ActiveWorkSnapshot } from "../Canvas/planView/activeWork";
import { resolveTabTabActivity } from "../Canvas/tabTabActivity";
import { openTeamPlanCanvas } from "../Canvas/workspacePlanEntry";
import { SortableContext, SortableRow, verticalListSortingStrategy, TAB_PREFIX, TEAM_PREFIX } from "./sortable";
import { buildSidebarDragModel, unitsInOrder, withSiblingOrder, workspaceTabOrder } from "./sidebarDragModel";
import { isWorkspaceTabActive } from "./sidebarSelection";
import { persistTabRename, SidebarSingleTabRow } from "./SidebarPlanPeerPair";
import { CursorTooltip } from "../shared/CursorTooltip";
import { TabItem, type TabStartedBy } from "./TabItem";
import { remoteTeamIdentity } from "./remoteOwnership";
import { useWorkspaceCrons } from "../../hooks/useWorkspaceCrons";
import { cronSelectionStore } from "../Cron/cronSelectionStore";
import { DropdownMenu } from "../shared/DropdownMenu";
import { RowMenuButton } from "../shared/RowMenuButton";
import { TeamJobsUnit } from "../Jobs/TeamJobsUnit";
import type { JobsTarget } from "../Jobs/JobsView";
import type { TabAttention } from "./tabAttention";
import { surfaceOwnerTitle } from "../../lib/surfaceTab";
import { cronReportTitle, terminalOrigin } from "./teamRowLabels";
import { SurfaceOwnerBlock } from "./SurfaceAttachments";
import { attachmentInset, ownerRowInset, surfaceAttachmentsByOwner, type OwnerRowContext } from "./surfaceAttachmentModel";

import {
  effectiveTeamId,
  visibleWorkspaceTabs,
  groupWorkspaceTeamMembers,
  partitionTeamMembers,
  type TeamTabMember,
} from "./workspaceTeamModel";
export type { TeamTabMember } from "./workspaceTeamModel";

type TeamPlan = Plan & { team_id?: string | null };
type TeamSection = { teamId: string; index: number };

function indexedPlan(entry: PlanIndexEntry): Plan {
  return {
    id: entry.plan_id,
    title: entry.title,
    sections: [],
    status: entry.status,
    owner: entry.owner,
    team_id: entry.team_id,
    project_path: entry.canonical_project_path ?? "",
    created_at: entry.updated_at,
    updated_at: entry.updated_at,
    milestone_kinds: [],
    loops: [],
    elements: [],
    tasks: [],
    canvas: { viewport: null },
    owner_history: [],
  };
}

function newestActivePlansByTeam(
  plans: readonly TeamPlan[],
  planIndex: readonly PlanIndexEntry[],
): Map<string, Plan> {
  const fullById = new Map(plans.map((plan) => [plan.id, plan]));
  const grouped = new Map<string, Plan>();
  const indexedIds = new Set(planIndex.map((entry) => entry.plan_id));
  const candidates = [
    ...planIndex.map((entry) => fullById.get(entry.plan_id) ?? indexedPlan(entry)),
    ...plans.filter((plan) => !indexedIds.has(plan.id)),
  ];
  for (const plan of candidates) {
    if (!plan.team_id || plan.status !== "active") continue;
    const current = grouped.get(plan.team_id);
    const currentUpdatedAt = current?.updated_at ?? current?.created_at ?? 0;
    const updatedAt = plan.updated_at ?? plan.created_at ?? 0;
    if (!current || updatedAt > currentUpdatedAt || (updatedAt === currentUpdatedAt && plan.id > current.id)) {
      grouped.set(plan.team_id, plan);
    }
  }
  return grouped;
}

/// Every role a Team with a spawn-capable manager can place.
const TEAM_MEMBER_ROLES = ["orchestrator", "planner", "cartographer", "worker"] as const;
/// With no spawner inside, only an anchoring role can be placed: it takes no
/// parent, so it is the door through which a stalled Team gets one.
const BOOTSTRAP_ONLY_ROLES = ["orchestrator"] as const;
/// A Team holds at most one live Orchestrator, so once it has one the role is
/// not on offer — the daemon refuses it by code either way.
const STAFFED_TEAM_ROLES = ["planner", "cartographer", "worker"] as const;

/// Mirrors the daemon headcount: a removed Orchestrator is gone from the tab
/// list and a dead one has had its agent identity cleared.
function hasLiveOrchestrator(members: readonly TeamTabMember[]): boolean {
  return members.some((member) => anchorsTeam(member.tab) && member.tab.agent_type != null);
}

export function addableTeamRoles(
  members: readonly TeamTabMember[],
  managerTabId: string | null,
): readonly TeamMemberRole[] {
  const hasCartographer = members.some(
    (member) => member.tab.role === "Manager" && roleKind(member.tab) === CARTOGRAPHER_KIND,
  );
  const withoutExistingCartographer = (roles: readonly TeamMemberRole[]): readonly TeamMemberRole[] =>
    hasCartographer ? roles.filter((role) => role !== "cartographer") : roles;
  if (hasLiveOrchestrator(members)) {
    return managerTabId ? withoutExistingCartographer(STAFFED_TEAM_ROLES) : [];
  }
  return managerTabId
    ? withoutExistingCartographer(TEAM_MEMBER_ROLES)
    : BOOTSTRAP_ONLY_ROLES;
}

export interface SidebarWorkspaceTabsProps {
  workspace: Workspace;
  activeTabId: string | null;
  sortedTabs: Tab[];
  tabs: Record<string, Tab>;
  tabStatuses: Record<string, TabStatus>;
  tabAttentionByTab?: Readonly<Record<string, TabAttention>>;
  teams: Team[];
  planIndex: PlanIndexEntry[];
  plans: Plan[];
  activeWork: ActiveWorkSnapshot | null;
  providers: ProviderDescriptor[];
  providersLoading?: boolean;
  closingTabIds: Set<string>;
  appearingTabIds: Set<string>;
  isActiveVisual: boolean;
  workspaceCanvasActive: boolean;
  touchMode: boolean;
  uiFontScale?: number;
  compact: boolean;
  sidebarActionText: string;
  orchestrators: Tab[];
  onBeforeOpenPlan: () => void;
  onActivateTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onRenameTab: (tabId: string, title: string) => void;
  onNewSession?: () => void;
  onOpenRemoteTab?: () => void;
  newSessionDisabled?: boolean;
  onRenameTeam?: (teamId: string, name: string, managerTabId: string | null) => void | Promise<void>;
  onAddTeamMember?: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    managerTabId: string | null,
    model?: string,
  ) => void | Promise<void>;
  onCloseTeam?: (teamId: string) => void;
  onOpenSentinelJob?: (target: JobsTarget) => void;
}

export function SidebarWorkspaceTabs({
  workspace: ws,
  activeTabId: wsActiveTabId,
  sortedTabs: allSortedTabs,
  tabs,
  tabStatuses,
  tabAttentionByTab = {},
  teams,
  planIndex,
  plans,
  activeWork,
  providers,
  providersLoading,
  closingTabIds,
  appearingTabIds,
  isActiveVisual,
  workspaceCanvasActive,
  touchMode,
  uiFontScale,
  compact,
  sidebarActionText,
  orchestrators,
  onBeforeOpenPlan,
  onActivateTab,
  onCloseTab,
  onRenameTab,
  onNewSession,
  onOpenRemoteTab,
  newSessionDisabled = false,
  onRenameTeam,
  onAddTeamMember,
  onCloseTeam,
  onOpenSentinelJob,
}: SidebarWorkspaceTabsProps) {
  const { jobs: workspaceCrons, refresh: refreshCrons } = useWorkspaceCrons(ws.id);
  const sortedTabs = useMemo(
    () => visibleWorkspaceTabs(allSortedTabs, tabs),
    [allSortedTabs, tabs],
  );
  const membersByTeam = useMemo(() => groupWorkspaceTeamMembers(sortedTabs), [sortedTabs]);
  // Workspace-wide so a surface follows its owner into a Team card even when the surface carries no team.
  const attachmentsByOwner = useMemo(() => surfaceAttachmentsByOwner(sortedTabs), [sortedTabs]);
  const dragModel = useMemo(
    () => buildSidebarDragModel(sortedTabs, attachmentsByOwner),
    [sortedTabs, attachmentsByOwner],
  );
  const { attachedIds, looseTabs, looseChildIds } = dragModel;

  const remoteTeams = useMemo(() => {
    const map = new Map<string, Team>();
    for (const tab of sortedTabs) {
      const identity = remoteTeamIdentity(tab);
      if (identity && !map.has(identity.id)) {
        map.set(identity.id, {
          id: identity.id,
          name: identity.title,
          lifecycle: "active",
          created_at: 0,
          workspace_id: ws.id,
          remote_peer_name: identity.peerName,
        });
      }
    }
    return map;
  }, [sortedTabs, ws.id]);

  const teamById = useMemo(
    () => new Map(teams.filter((team) => team.workspace_id === ws.id).map((team) => [team.id, team])),
    [teams, ws.id],
  );
  const plansByTeam = useMemo(
    () => newestActivePlansByTeam(plans as TeamPlan[], planIndex),
    [plans, planIndex],
  );

  const sharedRow = {
    workspaceId: ws.id,
    activeTabId: wsActiveTabId,
    isActiveVisual,
    workspaceCanvasActive,
    tabStatuses,
    tabAttentionByTab,
    orchestrators,
    touchMode,
    uiFontScale,
    onActivateTab,
    onCloseTab,
    onRenameTab: (tabId: string, title: string) => {
      onRenameTab(tabId, title);
      persistTabRename(ws.id, tabId, title);
    },
  };

  const renderOwnerWithAttachments = (
    owner: Tab,
    attachments: readonly Tab[],
    context: OwnerRowContext,
    renderOwner: Parameters<typeof SurfaceOwnerBlock>[0]["renderOwner"],
  ): ReactNode => {
    const inset = attachmentInset(ownerRowInset(context, touchMode));
    return (
      <SurfaceOwnerBlock
        ownerId={owner.id}
        attachments={attachments}
        compact={compact}
        renderOwner={renderOwner}
        wrapAttachments={(rows) => (
          <SortableContext items={attachments.map((surface) => `${TAB_PREFIX}${surface.id}`)} strategy={verticalListSortingStrategy}>
            {rows}
          </SortableContext>
        )}
        renderAttachment={(surface, surfaceIndex) => (
          <SidebarSingleTabRow
            tab={surface}
            ownerTitle={surfaceOwnerTitle(surface, tabs)}
            index={surfaceIndex}
            activity={resolveTabTabActivity(planIndex, surface.id, tabs)}
            activeWork={activeWork}
            isClosing={closingTabIds.has(surface.id)}
            isAppearing={appearingTabIds.has(surface.id)}
            group="child"
            groupKey={owner.id}
            pinnedIds={[]}
            toWorkspaceOrder={reorderSiblings}
            variant="attachment"
            insetPx={inset}
            ownerBadgeTab={compact ? owner : null}
            {...sharedRow}
          />
        )}
      />
    );
  };

  const startedByOf = (tab: Tab): TabStartedBy => {
    const creatorId = tab.spawned_by_tab_id ?? null;
    const creator = creatorId ? tabs[creatorId] : null;
    const origin = terminalOrigin(creatorId, creator?.title, Boolean(creator));
    if (origin.kind === "tab") return { id: origin.id, label: origin.label, onOpen: () => onActivateTab(origin.id) };
    if (origin.kind === "closed") return { id: origin.id, label: "a closed tab" };
    return { id: null, label: "an unknown tab" };
  };

  const renderMemberRow = (member: TeamTabMember, section: TeamSection, startedBy?: TabStartedBy): ReactNode => (
    <div key={member.tab.id} className="sidebar-team-member">
      {renderOwnerWithAttachments(member.tab, attachmentsByOwner.get(member.tab.id) ?? [], "team", (toggle) => (
        <SidebarSingleTabRow
          tab={member.tab}
          ownerTitle={surfaceOwnerTitle(member.tab, tabs)}
          index={member.index}
          activity={resolveTabTabActivity(planIndex, member.tab.id, tabs)}
          activeWork={activeWork}
          isClosing={closingTabIds.has(member.tab.id)}
          isAppearing={appearingTabIds.has(member.tab.id)}
          group="team"
          groupKey={`${section.teamId}:${section.index}`}
          pinnedIds={[]}
          toWorkspaceOrder={reorderSiblings}
          attachmentToggle={toggle}
          startedBy={startedBy}
          {...sharedRow}
        />
      ))}
    </div>
  );

  const renderTerminalRow = (member: TeamTabMember, section: TeamSection): ReactNode => renderMemberRow(member, section, startedByOf(member.tab));

  const renderedTeamIds = new Set<string>();
  const rows: ReactNode[] = [];
  const workspaceTabIds = allSortedTabs.map((tab) => tab.id);
  const unitItems = dragModel.units.map((unit) => `${unit.kind === "team" ? TEAM_PREFIX : TAB_PREFIX}${unit.key}`);
  const reorderUnits = (orderedKeys: string[]) => ({
    workspaceId: ws.id,
    orderedTabIds: workspaceTabOrder(workspaceTabIds, unitsInOrder(dragModel.units, orderedKeys)),
  });
  function reorderSiblings(ordered: string[]) {
    return {
      workspaceId: ws.id,
      orderedTabIds: workspaceTabOrder(workspaceTabIds, withSiblingOrder(dragModel.units, ordered, dragModel.blockOf)),
    };
  }

  const pinnedLooseItems = looseTabs
    .filter(
      (tab) =>
        !looseChildIds.has(tab.id) && tab.protected,
    )
    .map((tab) => `${TAB_PREFIX}${tab.id}`);

  const renderLooseTabTree = (
    tab: Tab,
    index: number,
    group: "tab" | "child",
    groupKey: string,
    pinnedIds: string[],
  ): ReactNode => {
    const children = looseTabs.filter((candidate) => candidate.parent_tab_id === tab.id);
    const childItems = children.map((child) => `${TAB_PREFIX}${child.id}`);
    return (
      <div key={tab.id} className="sidebar-tab-tree-row">
        {renderOwnerWithAttachments(tab, attachmentsByOwner.get(tab.id) ?? [], "loose", (toggle) => (
          <SidebarSingleTabRow
            tab={tab}
            ownerTitle={surfaceOwnerTitle(tab, tabs)}
            index={index}
            activity={resolveTabTabActivity(planIndex, tab.id, tabs)}
            activeWork={activeWork}
            isClosing={closingTabIds.has(tab.id)}
            isAppearing={appearingTabIds.has(tab.id)}
            group={group}
            groupKey={groupKey}
            pinnedIds={pinnedIds}
            toWorkspaceOrder={group === "tab" ? reorderUnits : reorderSiblings}
            attachmentToggle={toggle}
            {...sharedRow}
          />
        ))}
        {children.length > 0 && (
          <SortableContext items={childItems} strategy={verticalListSortingStrategy}>
            {children.map((child, childIndex) =>
              renderLooseTabTree(child, childIndex, "child", tab.id, []),
            )}
          </SortableContext>
        )}
      </div>
    );
  };

  for (const [index, tab] of sortedTabs.entries()) {
    if (attachedIds.has(tab.id)) continue;
    const teamId = effectiveTeamId(tab);
    if (!teamId) {
      if (!looseChildIds.has(tab.id)) {
        rows.push(renderLooseTabTree(tab, index, "tab", ws.id, pinnedLooseItems));
      }
      continue;
    }
    if (renderedTeamIds.has(teamId)) continue;
    renderedTeamIds.add(teamId);

    const members = (membersByTeam.get(teamId) ?? []).filter((member) => !attachedIds.has(member.tab.id));
    const { managers, terminals, workers, agents } = partitionTeamMembers(members);
    const managerItems = managers.map((member) => `${TAB_PREFIX}${member.tab.id}`);
    const workerItems = workers.map((member) => `${TAB_PREFIX}${member.tab.id}`);
    const agentItems = agents.map((member) => `${TAB_PREFIX}${member.tab.id}`);
    const terminalItems = terminals.map((member) => `${TAB_PREFIX}${member.tab.id}`);
    const workingLocal = workers.filter((member) => tabActivity(member.tab, tabStatuses[member.tab.id]).active).length;
    const team = teamById.get(teamId) ?? remoteTeams.get(teamId) ?? {
      id: teamId,
      name: null,
      lifecycle: "forming" as const,
      created_at: 0,
      workspace_id: ws.id,
    };
    const managerTabId = selectTeamStaffingManager(managers.map((member) => member.tab));
    const teamCrons = workspaceCrons.filter((job) => job.team_id === teamId);

    rows.push(
      <SortableRow
        key={teamId}
        id={`${TEAM_PREFIX}${teamId}`}
        data={{ group: "tab", groupKey: ws.id, pinnedIds: pinnedLooseItems, toWorkspaceOrder: reorderUnits }}
      >
      <TeamJobsUnit
        sentinelScope={onOpenSentinelJob && teamById.has(teamId) ? { workspace_id: ws.id, team_id: teamId } : undefined}
        onOpenSentinel={onOpenSentinelJob ? (scenarioId) => onOpenSentinelJob({ teamId, scenarioId }) : undefined}
        team={team}
        plan={plansByTeam.get(teamId) ?? null}
        tabs={tabs}
        managerCount={managers.length}
        managers={
          <SortableContext items={managerItems} strategy={verticalListSortingStrategy}>
            {managers.map((member) => renderMemberRow(member, { teamId, index: 0 }))}
          </SortableContext>
        }
        workerCount={workers.length}
        workingWorkerCount={workingLocal}
        workers={
          <>
            <SortableContext items={workerItems} strategy={verticalListSortingStrategy}>
              {workers.map((member) => renderMemberRow(member, { teamId, index: 1 }))}
            </SortableContext>
          </>
        }
        agentCount={agents.length}
        agents={
          <SortableContext items={agentItems} strategy={verticalListSortingStrategy}>
            {agents.map((member) => renderMemberRow(member, { teamId, index: 3 }))}
          </SortableContext>
        }
        terminalCount={terminals.length}
        terminals={
          <SortableContext items={terminalItems} strategy={verticalListSortingStrategy}>
            {terminals.map((member) => renderTerminalRow(member, { teamId, index: 2 }))}
          </SortableContext>
        }
        cronCount={teamCrons.length}
        crons={teamCrons.map((job) => (
          <div key={job.id} className="sidebar-team-cron-row">
            <button
              type="button"
              className="sidebar-team-cron-main"
              onClick={() => {
                onBeforeOpenPlan();
                cronSelectionStore.open({ cronId: job.id, workspaceId: ws.id });
              }}
              title={cronReportTitle(job.report_target_tab_id, job.report_target_tab_id ? tabs[job.report_target_tab_id]?.title : null)}
            >
              <Clock3 size={11} aria-hidden="true" />
              <span className="sidebar-team-cron-name">{job.name}</span>
              <span className="sidebar-team-cron-state">{job.enabled ? "active" : "paused"}</span>
            </button>
            <DropdownMenu
              className="sidebar-team-cron-menu"
              menuLabel="Cron actions"
              items={[{
                label: "Delete cron",
                icon: <X size={11} strokeWidth={2} />,
                danger: true,
                onClick: () => { void invoke("cron_delete", { id: job.id }).then(() => refreshCrons()); },
              }]}
              trigger={<RowMenuButton label={`Cron actions for ${job.name}`} title="Cron actions" />}
            />
          </div>
        ))}
        providers={providers}
        providersLoading={providersLoading}
        onRenameTeam={onRenameTeam
          ? (teamId, name) => onRenameTeam(teamId, name, managerTabId)
          : undefined}
        onOpenEffort={(target) => {
          onBeforeOpenPlan();
          openTeamPlanCanvas({ ...target, managerTabId });
        }}
        addableRoles={addableTeamRoles(members, managerTabId)}
        onAddMember={onAddTeamMember
          ? (role, providerId, model) => onAddTeamMember(teamId, role, providerId, managerTabId, model)
          : undefined}
        onCloseTeam={onCloseTeam}
      />
      </SortableRow>,
    );
  }

  return (
    <>
      <SortableContext items={unitItems} strategy={verticalListSortingStrategy}>
        {rows}
      </SortableContext>
      <div className="mx-2 mt-1 mb-0.5 flex min-w-0 items-center gap-1">
        {onNewSession && !ws.cwd_missing && <CursorTooltip
          active={newSessionDisabled}
          content="Finish Get started first"
          className="flex min-w-0 flex-1"
        >
          <button
            type="button"
            data-testid="sidebar-new-session"
            onClick={onNewSession}
            disabled={newSessionDisabled}
            aria-disabled={newSessionDisabled}
            className={`sidebar-new-session flex min-w-0 flex-1 items-center gap-2 px-3 ${touchMode ? "min-h-[44px] py-2" : "py-1"} rounded-md text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] disabled:opacity-40 disabled:hover:bg-transparent transition-colors`}
            // The cursor tip is the visual affordance; the control still carries
            // its own explanation, which is what a reader and a test can see.
            title={
              newSessionDisabled
                ? "Finish Get started before creating another session"
                : "New session in this workspace"
            }
          >
            <Plus size={10} strokeWidth={1.5} className="shrink-0" />
            <span className={`${sidebarActionText} min-w-0 truncate`}>New session</span>
          </button>
        </CursorTooltip>}
        {onOpenRemoteTab && (
          <button type="button" onClick={onOpenRemoteTab} aria-label="Open remote Tab" title="Open remote Tab" className={`flex shrink-0 items-center justify-center rounded-md text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-muted-fg)] ${touchMode ? "h-11 w-11" : "h-6 w-6"}`}>
            <MonitorUp size={12} strokeWidth={1.5} />
          </button>
        )}
      </div>
    </>
  );
}

export interface SidebarCollapsedActiveTabProps {
  workspace: Workspace;
  activeTabId: string | null;
  tabs: Record<string, Tab>;
  tabStatuses: Record<string, TabStatus>;
  tabAttentionByTab?: Readonly<Record<string, TabAttention>>;
  planIndex: PlanIndexEntry[];
  isActiveVisual: boolean;
  workspaceCanvasActive: boolean;
  orchestrators: Tab[];
  touchMode: boolean;
  uiFontScale?: number;
  onActivateTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onRenameTab: (tabId: string, title: string) => void;
}

export function SidebarCollapsedActiveTab({
  workspace: ws,
  activeTabId,
  tabs,
  tabStatuses,
  tabAttentionByTab = {},
  planIndex,
  isActiveVisual,
  workspaceCanvasActive,
  orchestrators,
  touchMode,
  uiFontScale,
  onActivateTab,
  onCloseTab,
  onRenameTab,
}: SidebarCollapsedActiveTabProps) {
  if (!activeTabId) return null;
  if (!ws.tabs.includes(activeTabId)) return null;
  const activeTab = tabs[activeTabId];
  if (!activeTab) return null;
  const activity = resolveTabTabActivity(planIndex, activeTab.id, tabs);

  return (
    <TabItem
      tab={activeTab}
      ownerTitle={surfaceOwnerTitle(activeTab, tabs)}
      index={0}
      isActive={isWorkspaceTabActive(
        isActiveVisual,
        workspaceCanvasActive,
        activeTab.id,
        activeTabId ?? undefined,
      )}
      onClick={() => onActivateTab(activeTab.id)}
      onClose={() => onCloseTab(activeTab.id)}
      canClose={true}
      workspaceId={ws.id}
      hasAgent={Boolean(activeTab.agent_type)}
      status={tabStatuses[activeTab.id] ?? null}
      attention={tabAttentionByTab[activeTab.id] ?? null}
      onRename={(title) => {
        onRenameTab(activeTab.id, title);
        persistTabRename(ws.id, activeTab.id, title);
      }}
      orchestrators={orchestrators}
      touchMode={touchMode}
      uiFontScale={uiFontScale}
      activity={activity}
    />
  );
}
