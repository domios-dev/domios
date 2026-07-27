import { useMemo, type ReactNode } from "react";
import { Plus } from "lucide-react";
import type {
  Tab,
  TabStatus,
  ProviderDescriptor,
  RemoteWorkersByOrchestrator,
  Team,
  Workspace,
} from "../../lib/types";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Plan } from "../Canvas/types";
import type { ActiveWorkSnapshot } from "../Canvas/planView/activeWork";
import { resolveTabTabActivity } from "../Canvas/tabTabActivity";
import { openTeamPlanCanvas } from "../Canvas/workspacePlanEntry";
import { RemoteWorkerRow } from "./RemoteWorkerRow";
import { SortableContext, verticalListSortingStrategy, TAB_PREFIX } from "./sortable";
import { isWorkspaceTabActive } from "./sidebarSelection";
import { persistTabRename, SidebarSingleTabRow } from "./SidebarPlanPeerPair";
import { sortTeamManagers } from "./orchestrationSections";
import { TabItem } from "./TabItem";
import { isSidebarTabVisible } from "./workspaceTabs";
import {
  TeamUnit,
  type TeamMemberRole,
} from "./TeamUnit";

type TeamTab = Tab & { team_id?: string | null };
type TeamPlan = Plan & { team_id?: string | null };

function newestActivePlansByTeam(plans: readonly TeamPlan[]): Map<string, Plan> {
  const grouped = new Map<string, Plan>();
  for (const plan of plans) {
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

interface TeamTabMember {
  tab: Tab;
  index: number;
}

function isEligibleTeamStaffingManager(member: TeamTabMember): boolean {
  return member.tab.role === "Orchestrator"
    || (member.tab.role === "Manager" && member.tab.role_profile === "planner");
}

function selectTeamStaffingManager(members: readonly TeamTabMember[]): string | null {
  return members.find(isEligibleTeamStaffingManager)?.tab.id ?? null;
}

export interface SidebarWorkspaceTabsProps {
  workspace: Workspace;
  activeTabId: string | null;
  sortedTabs: Tab[];
  tabs: Record<string, Tab>;
  tabStatuses: Record<string, TabStatus>;
  teams: Team[];
  planIndex: PlanIndexEntry[];
  plans: Plan[];
  activeWork: ActiveWorkSnapshot | null;
  remoteWorkers: RemoteWorkersByOrchestrator;
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
  newSessionDisabled?: boolean;
  onRenameTeam?: (teamId: string, name: string, managerTabId: string) => void | Promise<void>;
  onAddTeamMember?: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    managerTabId: string,
    model?: string,
  ) => void | Promise<void>;
  onCloseTeam?: (teamId: string, managerTabId: string) => void;
}

export function SidebarWorkspaceTabs({
  workspace: ws,
  activeTabId: wsActiveTabId,
  sortedTabs: allSortedTabs,
  tabs,
  tabStatuses,
  teams,
  planIndex,
  plans,
  activeWork,
  remoteWorkers,
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
  newSessionDisabled = false,
  onRenameTeam,
  onAddTeamMember,
  onCloseTeam,
}: SidebarWorkspaceTabsProps) {
  const sortedTabs = useMemo(
    () => allSortedTabs.filter(isSidebarTabVisible),
    [allSortedTabs],
  );
  const membersByTeam = useMemo(() => {
    const grouped = new Map<string, TeamTabMember[]>();
    sortedTabs.forEach((tab, index) => {
      const teamTab = tab as TeamTab;
      if (!teamTab.team_id) return;
      const members = grouped.get(teamTab.team_id) ?? [];
      members.push({ tab: teamTab, index });
      grouped.set(teamTab.team_id, members);
    });
    return grouped;
  }, [sortedTabs, tabs]);

  const teamById = useMemo(
    () => new Map(teams.filter((team) => team.workspace_id === ws.id).map((team) => [team.id, team])),
    [teams, ws.id],
  );
  const plansByTeam = useMemo(() => newestActivePlansByTeam(plans as TeamPlan[]), [plans]);

  const sharedRow = {
    workspaceId: ws.id,
    activeTabId: wsActiveTabId,
    isActiveVisual,
    workspaceCanvasActive,
    tabStatuses,
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
    items: string[],
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
        items={items}
        pinnedIds={[]}
        {...sharedRow}
      />
    </div>
  );

  const renderedTeamIds = new Set<string>();
  const rows: ReactNode[] = [];
  const looseItems = sortedTabs
    .filter((tab) => !(tab as TeamTab).team_id)
    .map((tab) => `${TAB_PREFIX}${tab.id}`);
  const pinnedLooseItems = sortedTabs
    .filter(
      (tab) =>
        !(tab as TeamTab).team_id && tab.protected,
    )
    .map((tab) => `${TAB_PREFIX}${tab.id}`);

  for (const [index, tab] of sortedTabs.entries()) {
    const teamTab = tab as TeamTab;
    const teamId = teamTab.team_id ?? null;
    if (!teamId) {
      rows.push(
        <SidebarSingleTabRow
          key={teamTab.id}
          tab={teamTab}
          index={index}
          activity={resolveTabTabActivity(planIndex, teamTab.id, tabs)}
          activeWork={activeWork}
          isClosing={closingTabIds.has(teamTab.id)}
          isAppearing={appearingTabIds.has(teamTab.id)}
          group="tab"
          groupKey={ws.id}
          items={looseItems}
          pinnedIds={pinnedLooseItems}
          {...sharedRow}
        />,
      );
      continue;
    }
    if (renderedTeamIds.has(teamId)) continue;
    renderedTeamIds.add(teamId);

    const members = membersByTeam.get(teamId) ?? [];
    const managers = sortTeamManagers(
      members.filter((member) => member.tab.role === "Orchestrator" || member.tab.role === "Manager"),
    );
    const workers = members.filter(
      (member) => member.tab.role !== "Orchestrator" && member.tab.role !== "Manager",
    );
    const managerItems = managers.map((member) => `${TAB_PREFIX}${member.tab.id}`);
    const workerItems = workers.map((member) => `${TAB_PREFIX}${member.tab.id}`);
    const orchestratorIds = managers
      .filter((member) => member.tab.role === "Orchestrator")
      .map((member) => member.tab.id);
    const teamRemoteWorkers = orchestratorIds.flatMap((id) => remoteWorkers[id] ?? []);
    const workingLocal = workers.filter((member) => tabStatuses[member.tab.id] === "Working").length;
    const workingRemote = teamRemoteWorkers.filter((worker) => worker.status === "working").length;
    const team = teamById.get(teamId) ?? {
      id: teamId,
      name: null,
      lifecycle: "forming" as const,
      created_at: 0,
      workspace_id: ws.id,
    };
    const managerTabId = selectTeamStaffingManager(managers);

    rows.push(
      <TeamUnit
        key={teamId}
        team={team}
        plan={plansByTeam.get(teamId) ?? null}
        tabs={tabs}
        managerCount={managers.length}
        managers={
          <SortableContext items={managerItems} strategy={verticalListSortingStrategy}>
            {managers.map((member) => renderMemberRow(member, managerItems))}
          </SortableContext>
        }
        workerCount={workers.length + teamRemoteWorkers.length}
        workingWorkerCount={workingLocal + workingRemote}
        workers={
          <>
            <SortableContext items={workerItems} strategy={verticalListSortingStrategy}>
              {workers.map((member) => renderMemberRow(member, workerItems))}
            </SortableContext>
            {teamRemoteWorkers.map((worker) => (
              <RemoteWorkerRow
                key={`${worker.peer}/${worker.remote_tab}`}
                worker={worker}
                depth={0}
              />
            ))}
          </>
        }
        providers={providers}
        providersLoading={providersLoading}
        onRenameTeam={onRenameTeam && managerTabId
          ? (teamId, name) => onRenameTeam(teamId, name, managerTabId)
          : undefined}
        onOpenEffort={(target) => {
          onBeforeOpenPlan();
          openTeamPlanCanvas({ ...target, managerTabId });
        }}
        onAddMember={onAddTeamMember && managerTabId
          ? (role, providerId, model) => onAddTeamMember(teamId, role, providerId, managerTabId, model)
          : undefined}
        onCloseTeam={onCloseTeam && managerTabId
          ? (teamId) => onCloseTeam(teamId, managerTabId)
          : undefined}
      />,
    );
  }

  return (
    <>
      {rows}
      <button
        type="button"
        data-testid="sidebar-new-session"
        onClick={onNewSession}
        disabled={newSessionDisabled}
        aria-disabled={newSessionDisabled}
        className={`sidebar-new-session flex items-center gap-2 min-w-0 max-w-full mx-2 mt-1 mb-0.5 px-3 ${touchMode ? "min-h-[44px] py-2" : "py-1"} rounded-md text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] disabled:opacity-40 disabled:hover:bg-transparent transition-colors`}
        title={newSessionDisabled ? "Finish Get started before creating another session" : "New session in this workspace"}
      >
        <Plus size={10} strokeWidth={1.5} className="shrink-0" />
        <span className={`${sidebarActionText} min-w-0 truncate`}>New session</span>
      </button>
    </>
  );
}

export interface SidebarCollapsedActiveTabProps {
  workspace: Workspace;
  activeTabId: string | null;
  tabs: Record<string, Tab>;
  tabStatuses: Record<string, TabStatus>;
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
