import { useMemo, type ReactNode } from "react";
import { Plus } from "lucide-react";
import type {
  Pane,
  PaneStatus,
  ProviderDescriptor,
  RemoteWorkersByOrchestrator,
  Tab,
  Team,
  Workspace,
} from "../../lib/types";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Plan } from "../Canvas/types";
import type { ActiveWorkSnapshot } from "../Canvas/planView/activeWork";
import { resolvePaneTabActivity } from "../Canvas/paneTabActivity";
import { openTeamPlanCanvas } from "../Canvas/workspacePlanEntry";
import { RemoteWorkerRow } from "./RemoteWorkerRow";
import { SortableContext, verticalListSortingStrategy, TAB_PREFIX } from "./sortable";
import { collectPaneIds } from "./workspaceTabs";
import { isWorkspaceTabActive } from "./sidebarSelection";
import { persistTabRename, SidebarSingleTabRow } from "./SidebarPlanPeerPair";
import { sortTeamManagers } from "./orchestrationSections";
import { TabItem } from "./TabItem";
import { aggregateTabStatus } from "./sidebarTabStatus";
import {
  TeamUnit,
  type TeamMemberRole,
} from "./TeamUnit";

type TeamPane = Pane & { team_id?: string | null };
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
  pane: TeamPane;
  index: number;
}

function isEligibleTeamStaffingManager(member: TeamTabMember): boolean {
  return member.pane.role === "Orchestrator"
    || (member.pane.role === "Manager" && member.pane.role_profile === "planner");
}

function selectTeamStaffingManager(members: readonly TeamTabMember[]): string | null {
  return members.find(isEligibleTeamStaffingManager)?.pane.id ?? null;
}

export interface SidebarWorkspaceTabsProps {
  workspace: Workspace;
  activeTabId: string | null;
  sortedTabs: Tab[];
  panes: Record<string, Pane>;
  paneStatuses: Record<string, PaneStatus>;
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
  orchestrators: Pane[];
  onBeforeOpenPlan: () => void;
  onActivatepane: (tabId: string) => void;
  onClosepane: (tabId: string) => void;
  onRenamepane: (tabId: string, title: string) => void;
  onNewSession: () => void;
  onRenameTeam?: (teamId: string, name: string, managerPaneId: string) => void | Promise<void>;
  onAddTeamMember?: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    managerPaneId: string,
    model?: string,
  ) => void | Promise<void>;
  onCloseTeam?: (teamId: string, managerPaneId: string) => void;
}

export function SidebarWorkspaceTabs({
  workspace: ws,
  activeTabId: wsActiveTabId,
  sortedTabs,
  panes,
  paneStatuses,
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
  onRenameTeam,
  onAddTeamMember,
  onCloseTeam,
}: SidebarWorkspaceTabsProps) {
  const membersByTeam = useMemo(() => {
    const grouped = new Map<string, TeamTabMember[]>();
    sortedTabs.forEach((tab, index) => {
      const pane = panes[tab.pane_id] as TeamPane | undefined;
      if (!pane?.team_id) return;
      const members = grouped.get(pane.team_id) ?? [];
      members.push({ tab, pane, index });
      grouped.set(pane.team_id, members);
    });
    return grouped;
  }, [sortedTabs, panes]);

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
    panes,
    paneStatuses,
    orchestrators,
    touchMode,
    uiFontScale,
    onActivateTab,
    onCloseTab,
    onRenamepane: (tabId: string, title: string) => {
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
        pane={member.pane}
        paneIds={collectPaneIds(member.tab, panes)}
        activity={resolvePaneTabActivity(planIndex, member.pane.id, panes)}
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
    .filter((tab) => !(panes[tab.pane_id] as TeamPane | undefined)?.team_id)
    .map((tab) => `${TAB_PREFIX}${tab.id}`);
  const pinnedLooseItems = sortedTabs
    .filter(
      (tab) =>
        !(panes[tab.pane_id] as TeamPane | undefined)?.team_id && panes[tab.pane_id]?.protected,
    )
    .map((tab) => `${TAB_PREFIX}${tab.id}`);

  for (const [index, tab] of sortedTabs.entries()) {
    const pane = panes[tab.pane_id] as TeamPane | undefined;
    const teamId = pane?.team_id ?? null;
    if (!teamId) {
      rows.push(
        <SidebarSingleTabRow
          key={tab.id}
          tab={tab}
          index={index}
          pane={pane ?? null}
          paneIds={collectPaneIds(tab, panes)}
          activity={pane ? resolvePaneTabActivity(planIndex, pane.id, panes) : null}
          activeWork={activeWork}
          isClosing={closingTabIds.has(tab.id)}
          isAppearing={appearingTabIds.has(tab.id)}
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
      members.filter((member) => member.pane.role === "Orchestrator" || member.pane.role === "Manager"),
    );
    const workers = members.filter(
      (member) => member.pane.role !== "Orchestrator" && member.pane.role !== "Manager",
    );
    const managerItems = managers.map((member) => `${TAB_PREFIX}${member.tab.id}`);
    const workerItems = workers.map((member) => `${TAB_PREFIX}${member.tab.id}`);
    const orchestratorIds = managers
      .filter((member) => member.pane.role === "Orchestrator")
      .map((member) => member.pane.id);
    const teamRemoteWorkers = orchestratorIds.flatMap((id) => remoteWorkers[id] ?? []);
    const workingLocal = workers.filter((member) => paneStatuses[member.pane.id] === "Working").length;
    const workingRemote = teamRemoteWorkers.filter((worker) => worker.status === "working").length;
    const team = teamById.get(teamId) ?? {
      id: teamId,
      name: null,
      lifecycle: "forming" as const,
      created_at: 0,
      workspace_id: ws.id,
    };
    const managerPaneId = selectTeamStaffingManager(managers);

    rows.push(
      <TeamUnit
        key={teamId}
        team={team}
        plan={plansByTeam.get(teamId) ?? null}
        panes={panes}
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
                key={`${worker.peer}/${worker.remote_pane}`}
                worker={worker}
                depth={0}
              />
            ))}
          </>
        }
        providers={providers}
        providersLoading={providersLoading}
        onRenameTeam={onRenameTeam && managerPaneId
          ? (teamId, name) => onRenameTeam(teamId, name, managerPaneId)
          : undefined}
        onOpenEffort={(target) => {
          onBeforeOpenPlan();
          openTeamPlanCanvas({ ...target, managerPaneId });
        }}
        onAddMember={onAddTeamMember && managerPaneId
          ? (role, providerId, model) => onAddTeamMember(teamId, role, providerId, managerPaneId, model)
          : undefined}
        onCloseTeam={onCloseTeam && managerPaneId
          ? (teamId) => onCloseTeam(teamId, managerPaneId)
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
        className={`sidebar-new-session flex items-center gap-2 min-w-0 max-w-full mx-2 mt-1 mb-0.5 px-3 ${touchMode ? "min-h-[44px] py-2" : "py-1"} rounded-md text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] disabled:opacity-40 disabled:hover:bg-transparent transition-colors`}
        title="New session in this workspace"
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
  panes: Record<string, Pane>;
  paneStatuses: Record<string, PaneStatus>;
  planIndex: PlanIndexEntry[];
  isActiveVisual: boolean;
  workspaceCanvasActive: boolean;
  orchestrators: Pane[];
  touchMode: boolean;
  uiFontScale?: number;
  onActivatepane: (tabId: string) => void;
  onClosepane: (tabId: string) => void;
  onRenamepane: (tabId: string, title: string) => void;
}

export function SidebarCollapsedActiveTab({
  workspace: ws,
  activeTabId,
  panes,
  paneStatuses,
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
  const activeTab = ws.tabs.find((tab) => tab.id === activeTabId);
  if (!activeTab) return null;
  const paneIds = collectPaneIds(activeTab, panes);
  const firstPane = paneIds[0] ? panes[paneIds[0]] : null;
  const activity = firstPane ? resolvePaneTabActivity(planIndex, firstPane.id, panes) : null;

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
      paneCount={paneIds.length}
      workspaceId={ws.id}
      hasAgent={paneIds.some((paneId) => Boolean(panes[paneId]?.agent_type))}
      status={aggregateTabStatus(paneIds, paneStatuses)}
      onRename={(title) => {
        onRenameTab(activeTab.id, title);
        persistTabRename(ws.id, activeTab.id, title);
      }}
      pane={firstPane}
      orchestrators={orchestrators}
      touchMode={touchMode}
      uiFontScale={uiFontScale}
      activity={activity}
    />
  );
}
