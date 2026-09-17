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
import { anchorsTeam } from "../../lib/roleKind";
import type { TeamMemberRole } from "../../lib/teamActions";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Plan } from "../Canvas/types";
import type { ActiveWorkSnapshot } from "../Canvas/planView/activeWork";
import { resolveTabTabActivity } from "../Canvas/tabTabActivity";
import { openTeamPlanCanvas } from "../Canvas/workspacePlanEntry";
import { SortableContext, verticalListSortingStrategy, TAB_PREFIX } from "./sortable";
import { isWorkspaceTabActive } from "./sidebarSelection";
import { persistTabRename, SidebarSingleTabRow } from "./SidebarPlanPeerPair";
import { CursorTooltip } from "../shared/CursorTooltip";
import { sortTeamManagers } from "./orchestrationSections";
import { TabItem } from "./TabItem";
import { remoteTeamIdentity } from "./remoteOwnership";
import { isSidebarTabVisible } from "./workspaceTabs";
import { useWorkspaceCrons } from "../../hooks/useWorkspaceCrons";
import { cronSelectionStore } from "../Cron/cronSelectionStore";
import { DropdownMenu } from "../shared/DropdownMenu";
import { TeamUnit } from "./TeamUnit";
import type { DeclaredAttentionLevel } from "./declaredAttention";

type TeamTab = Tab & { team_id?: string | null };
type TeamPlan = Plan & { team_id?: string | null };

function effectiveTeamId(tab: Tab): string | null {
  return remoteTeamIdentity(tab)?.id ?? (tab as TeamTab).team_id ?? null;
}

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

export interface TeamTabMember {
  tab: Tab;
  index: number;
}

function isEligibleTeamStaffingManager(member: TeamTabMember): boolean {
  return member.tab.can_spawn === true
    && member.tab.role === "Manager";
}

/// Every role a Team with a spawn-capable manager can place.
const TEAM_MEMBER_ROLES = ["orchestrator", "planner", "worker"] as const;
/// With no spawner inside, only an anchoring role can be placed: it takes no
/// parent, so it is the door through which a stalled Team gets one.
const BOOTSTRAP_ONLY_ROLES = ["orchestrator"] as const;
/// A Team holds at most one live Orchestrator, so once it has one the role is
/// not on offer — the daemon refuses it by code either way.
const STAFFED_TEAM_ROLES = ["planner", "worker"] as const;

function selectTeamStaffingManager(members: readonly TeamTabMember[]): string | null {
  return members.find(isEligibleTeamStaffingManager)?.tab.id ?? null;
}

/// Mirrors the daemon headcount: a removed Orchestrator is gone from the tab
/// list and a dead one has had its agent identity cleared.
function hasLiveOrchestrator(members: readonly TeamTabMember[]): boolean {
  return members.some((member) => anchorsTeam(member.tab) && member.tab.agent_type != null);
}

export function addableTeamRoles(
  members: readonly TeamTabMember[],
  managerTabId: string | null,
): readonly TeamMemberRole[] {
  if (hasLiveOrchestrator(members)) return managerTabId ? STAFFED_TEAM_ROLES : [];
  return managerTabId ? TEAM_MEMBER_ROLES : BOOTSTRAP_ONLY_ROLES;
}

export interface SidebarWorkspaceTabsProps {
  workspace: Workspace;
  activeTabId: string | null;
  sortedTabs: Tab[];
  tabs: Record<string, Tab>;
  tabStatuses: Record<string, TabStatus>;
  declaredAttentionByTab?: Readonly<Record<string, DeclaredAttentionLevel>>;
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
  onNewSession: () => void;
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
}

export function SidebarWorkspaceTabs({
  workspace: ws,
  activeTabId: wsActiveTabId,
  sortedTabs: allSortedTabs,
  tabs,
  tabStatuses,
  declaredAttentionByTab = {},
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
  compact: _compact,
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
}: SidebarWorkspaceTabsProps) {
  const { jobs: workspaceCrons, refresh: refreshCrons } = useWorkspaceCrons(ws.id);
  const sortedTabs = useMemo(
    () => allSortedTabs.filter(isSidebarTabVisible),
    [allSortedTabs],
  );
  const membersByTeam = useMemo(() => {
    const grouped = new Map<string, TeamTabMember[]>();
    sortedTabs.forEach((tab, index) => {
      const teamId = effectiveTeamId(tab);
      if (!teamId) return;
      const members = grouped.get(teamId) ?? [];
      members.push({ tab, index });
      grouped.set(teamId, members);
    });
    return grouped;
  }, [sortedTabs, tabs]);

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
    declaredAttentionByTab,
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

  const renderMemberRow = (
    member: TeamTabMember,
  ): ReactNode => (
    <div key={member.tab.id} className="sidebar-team-member">
      <SidebarSingleTabRow
        tab={member.tab}
        index={member.index}
        activity={resolveTabTabActivity(planIndex, member.tab.id, tabs)}
        activeWork={activeWork}
        isClosing={closingTabIds.has(member.tab.id)}
        isAppearing={appearingTabIds.has(member.tab.id)}
        group="tab"
        groupKey={ws.id}
        pinnedIds={[]}
        {...sharedRow}
      />
    </div>
  );

  const renderTerminalRow = (member: TeamTabMember): ReactNode => {
    const creatorId = member.tab.spawned_by_tab_id ?? null;
    const creator = creatorId ? tabs[creatorId] : null;
    return (
      <div key={member.tab.id} className="sidebar-team-member sidebar-team-terminal-member">
        <SidebarSingleTabRow
          tab={member.tab}
          index={member.index}
          activity={resolveTabTabActivity(planIndex, member.tab.id, tabs)}
          activeWork={activeWork}
          isClosing={closingTabIds.has(member.tab.id)}
          isAppearing={appearingTabIds.has(member.tab.id)}
          group="tab"
          groupKey={ws.id}
          pinnedIds={[]}
          {...sharedRow}
        />
        <div className="sidebar-team-terminal-origin">
          started by{" "}
          {creator ? (
            <button type="button" onClick={() => onActivateTab(creator.id)}>
              {creator.title || creator.id}
            </button>
          ) : (
            <span>{creatorId ? `${creatorId} · archived` : "unknown"}</span>
          )}
        </div>
      </div>
    );
  };

  const renderedTeamIds = new Set<string>();
  const rows: ReactNode[] = [];
  const looseTabs = sortedTabs.filter((tab) => !effectiveTeamId(tab));
  const looseIds = new Set(looseTabs.map((tab) => tab.id));
  const looseChildIds = new Set(
    looseTabs
      .filter((tab) => tab.parent_tab_id && looseIds.has(tab.parent_tab_id))
      .map((tab) => tab.id),
  );
  const looseItems = looseTabs
    .filter((tab) => !looseChildIds.has(tab.id))
    .map((tab) => `${TAB_PREFIX}${tab.id}`);
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
        <SidebarSingleTabRow
          tab={tab}
          index={index}
          activity={resolveTabTabActivity(planIndex, tab.id, tabs)}
          activeWork={activeWork}
          isClosing={closingTabIds.has(tab.id)}
          isAppearing={appearingTabIds.has(tab.id)}
          group={group}
          groupKey={groupKey}
          pinnedIds={pinnedIds}
          {...sharedRow}
        />
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
    const teamTab = tab as TeamTab;
    const teamId = effectiveTeamId(teamTab);
    if (!teamId) {
      if (!looseChildIds.has(teamTab.id)) {
        rows.push(renderLooseTabTree(teamTab, index, "tab", ws.id, pinnedLooseItems));
      }
      continue;
    }
    if (renderedTeamIds.has(teamId)) continue;
    renderedTeamIds.add(teamId);

    const members = membersByTeam.get(teamId) ?? [];
    const managers = sortTeamManagers(
      members.filter((member) => member.tab.role === "Manager"),
    );
    const terminals = members.filter((member) => member.tab.role == null);
    const workers = members.filter((member) =>
      !anchorsTeam(member.tab)
      && member.tab.role !== "Manager"
      && member.tab.role != null,
    );
    const managerItems = managers.map((member) => `${TAB_PREFIX}${member.tab.id}`);
    const workerItems = workers.map((member) => `${TAB_PREFIX}${member.tab.id}`);
    const terminalItems = terminals.map((member) => `${TAB_PREFIX}${member.tab.id}`);
    const workingLocal = workers.filter((member) => tabStatuses[member.tab.id] === "Working").length;
    const team = teamById.get(teamId) ?? remoteTeams.get(teamId) ?? {
      id: teamId,
      name: null,
      lifecycle: "forming" as const,
      created_at: 0,
      workspace_id: ws.id,
    };
    const managerTabId = selectTeamStaffingManager(managers);
    const teamCrons = workspaceCrons.filter((job) => job.team_id === teamId);

    rows.push(
      <TeamUnit
        key={teamId}
        team={team}
        plan={plansByTeam.get(teamId) ?? null}
        tabs={tabs}
        managerCount={managers.length}
        managers={
          <SortableContext items={managerItems} strategy={verticalListSortingStrategy}>
            {managers.map((member) => renderMemberRow(member))}
          </SortableContext>
        }
        workerCount={workers.length}
        workingWorkerCount={workingLocal}
        workers={
          <>
            <SortableContext items={workerItems} strategy={verticalListSortingStrategy}>
              {workers.map((member) => renderMemberRow(member))}
            </SortableContext>
          </>
        }
        terminalCount={terminals.length}
        terminals={
          <SortableContext items={terminalItems} strategy={verticalListSortingStrategy}>
            {terminals.map((member) => renderTerminalRow(member))}
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
              title={job.report_target_tab_id
                ? `Reports to Tab ${job.report_target_tab_id}`
                : "Runs in a private cron worker"}
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
              trigger={
                <button
                  type="button"
                  aria-label={`Cron actions for ${job.name}`}
                  aria-haspopup="menu"
                  title="Cron actions"
                  className="flex h-5 w-5 items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-border)] hover:text-[var(--ct-fg)]"
                >
                  <span className="text-[13px] font-bold leading-none">⋮</span>
                </button>
              }
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
      />,
    );
  }

  return (
    <>
      <SortableContext items={looseItems} strategy={verticalListSortingStrategy}>
        {rows}
      </SortableContext>
      <div className="mx-2 mt-1 mb-0.5 flex min-w-0 items-center gap-1">
        <CursorTooltip
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
        </CursorTooltip>
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
  declaredAttentionByTab?: Readonly<Record<string, DeclaredAttentionLevel>>;
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
  declaredAttentionByTab = {},
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
      declaredAttention={declaredAttentionByTab[activeTab.id] ?? null}
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
