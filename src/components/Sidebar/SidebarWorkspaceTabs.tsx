import { tabActivity } from "../../lib/tabActivity";
import { useMemo, type ReactNode } from "react";
import { MonitorUp, Plus } from "lucide-react";
import type {
  Tab,
  TabStatus,
  ProviderDescriptor,
  Team,
  Workspace,
} from "../../lib/types";
import { selectTeamStaffingManager, type TeamMemberRole } from "../../lib/teamActions";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Plan } from "../Canvas/types";
import { resolveTabTabActivity } from "../Canvas/tabTabActivity";
import { openTeamPlanCanvas } from "../Canvas/workspacePlanEntry";
import { SortableContext, SortableRow, verticalListSortingStrategy, TAB_PREFIX, TEAM_PREFIX } from "./sortable";
import { buildSidebarDragModel, localWorkspaceTabIds, unitsInOrder, withSiblingOrder, workspaceTabOrder } from "./sidebarDragModel";
import { isWorkspaceTabActive } from "./sidebarSelection";
import { CursorTooltip } from "../shared/CursorTooltip";
import { TabItem } from "./TabItem";
import { remoteTeamIdentity } from "./remoteOwnership";
import { useWorkspaceCrons } from "../../hooks/useWorkspaceCrons";
import { useLatestCallback } from "../../hooks/useLatestCallback";
import { TeamJobsUnit } from "../Jobs/TeamJobsUnit";
import type { JobsTarget } from "../Jobs/JobsView";
import type { TabAttention } from "./tabAttention";
import { surfaceOwnerTitle } from "../../lib/surfaceTab";
import { SidebarCronRow } from "./SidebarResourceRow";
import { SidebarShellRow, SidebarOwnerTabRow, SidebarSortableBlock } from "./SidebarShellRow";
import { surfaceAttachmentsByOwner } from "./surfaceAttachmentModel";
import { AgentChecklistDisclosure, type ChecklistToggle } from "./AgentChecklist";
import { showsChecklist } from "./agentChecklistModel";
import { useNeighborGatewayIds } from "../Neighbors/neighborGateways";

import {
  addableTeamRoles,
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
  onRenamePlan?: (planId: string, title: string) => Promise<void>;
  onAddTeamMember?: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    managerTabId: string | null,
    model?: string,
  ) => void | Promise<void>;
  onCloseTeam?: (teamId: string) => void;
  closingTeamIds?: ReadonlySet<string>;
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
  onRenamePlan,
  onAddTeamMember,
  onCloseTeam,
  closingTeamIds,
  onOpenSentinelJob,
}: SidebarWorkspaceTabsProps) {
  const { jobs: workspaceCrons, refresh: refreshCrons } = useWorkspaceCrons(ws.id);
  const conciergeIds = useNeighborGatewayIds();
  // A tab bound to a Neighbor lives under that Neighbor, wherever it was created.
  const sortedTabs = useMemo(
    () => visibleWorkspaceTabs(allSortedTabs, tabs).filter((tab) => !conciergeIds.has(tab.id)),
    [allSortedTabs, tabs, conciergeIds],
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

  // Memoized rows compare handlers by identity; these always reach the current props.
  const activateRowTab = useLatestCallback(onActivateTab, []);
  const closeRowTab = useLatestCallback(onCloseTab, []);
  const renameRowTab = useLatestCallback(onRenameTab, []);
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
    onActivateTab: activateRowTab,
    onCloseTab: closeRowTab,
    onRenameTab: renameRowTab,
  };

  const renderMemberShellRow = (
    member: TeamTabMember,
    section: TeamSection,
    shell: boolean,
    checklist?: { toggle: ChecklistToggle; detail: ReactNode },
  ): ReactNode => (
    <SidebarShellRow key={member.tab.id}
      tab={member.tab} index={member.index} showStartedBy={shell}
      checklistToggle={checklist?.toggle} detail={checklist?.detail}
      attachments={attachmentsByOwner.get(member.tab.id) ?? []}
      tabs={tabs} planIndex={planIndex}
      closingTabIds={closingTabIds} appearingTabIds={appearingTabIds}
      compact={compact} group="team" groupKey={`${section.teamId}:${section.index}`}
      pinnedIds={[]} toWorkspaceOrder={reorderSiblings} {...sharedRow}
    />
  );

  // One element shape whether or not a checklist exists, so its first todo never remounts the row.
  const renderMemberRow = (member: TeamTabMember, section: TeamSection, shell = false): ReactNode => (
    <AgentChecklistDisclosure plans={plans} key={member.tab.id} tabId={member.tab.id} title={member.tab.title}>
      {(toggle, detail) => renderMemberShellRow(member, section, shell, showsChecklist(member.tab) ? { toggle, detail } : undefined)}
    </AgentChecklistDisclosure>
  );

  const renderTerminalRow = (member: TeamTabMember, section: TeamSection): ReactNode => renderMemberRow(member, section, true);

  const renderedTeamIds = new Set<string>();
  const rows: ReactNode[] = [];
  const workspaceTabIds = localWorkspaceTabIds(allSortedTabs);
  const unitItems = dragModel.units.map((unit) => `${unit.kind === "team" ? TEAM_PREFIX : TAB_PREFIX}${unit.key}`);
  const reorderUnits = useLatestCallback((orderedKeys: string[]) => ({
    workspaceId: ws.id,
    orderedTabIds: workspaceTabOrder(workspaceTabIds, unitsInOrder(dragModel.units, orderedKeys)),
  }), []);
  const reorderSiblings = useLatestCallback((ordered: string[]) => ({
    workspaceId: ws.id,
    orderedTabIds: workspaceTabOrder(workspaceTabIds, withSiblingOrder(dragModel.units, ordered, dragModel.blockOf)),
  }), []);

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
    const toWorkspaceOrder = group === "tab" ? reorderUnits : reorderSiblings;
    const renderOwner = (checklistToggle?: ChecklistToggle) => (
      <SidebarOwnerTabRow
        tab={tab} index={index} attachments={attachmentsByOwner.get(tab.id) ?? []} context="loose"
        tabs={tabs} planIndex={planIndex}
        closingTabIds={closingTabIds} appearingTabIds={appearingTabIds}
        compact={compact} group="team" groupKey={tab.id} pinnedIds={[]}
        toWorkspaceOrder={reorderSiblings} ownerSortable={false} checklistToggle={checklistToggle} {...sharedRow}
      />
    );
    return (
      <SidebarSortableBlock key={tab.id} tab={tab} group={group} groupKey={groupKey} pinnedIds={pinnedIds}
        toWorkspaceOrder={toWorkspaceOrder} className="sidebar-tab-tree-row">
        <AgentChecklistDisclosure plans={plans} tabId={tab.id} title={tab.title}>
          {(toggle, detail) => <>{renderOwner(showsChecklist(tab) ? toggle : undefined)}{showsChecklist(tab) && detail}</>}
        </AgentChecklistDisclosure>
        {children.length > 0 && (
          <SortableContext items={childItems} strategy={verticalListSortingStrategy}>
            {children.map((child, childIndex) =>
              renderLooseTabTree(child, childIndex, "child", tab.id, []),
            )}
          </SortableContext>
        )}
      </SidebarSortableBlock>
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
    const plan = plansByTeam.get(teamId) ?? null;
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
        plan={plan}
        tabs={tabs}
        workingManagerCount={managers.filter(member => tabActivity(member.tab, tabStatuses[member.tab.id]).active).length}
        workingAgentCount={agents.filter(member => tabActivity(member.tab, tabStatuses[member.tab.id]).active).length}
        workingTerminalCount={terminals.filter(member => tabActivity(member.tab, tabStatuses[member.tab.id]).active).length}
        workingCronCount={teamCrons.filter(job => job.last_run?.status === "Running" || job.last_run?.status === "AwaitingOwnerPrompt").length}
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
          <SidebarCronRow key={job.id} job={job} tabs={tabs}
            onBeforeOpen={onBeforeOpenPlan} onDeleted={refreshCrons} />
        ))}
        providers={providers}
        providersLoading={providersLoading}
        onRenameTeam={plan
          ? onRenamePlan ? (_teamId, name) => onRenamePlan(plan.id, name) : undefined
          : onRenameTeam ? (teamId, name) => onRenameTeam(teamId, name, managerTabId) : undefined}
        onOpenEffort={(target) => {
          onBeforeOpenPlan();
          openTeamPlanCanvas({ ...target, managerTabId });
        }}
        addableRoles={addableTeamRoles(members, managerTabId)}
        onAddMember={onAddTeamMember
          ? (role, providerId, model) => onAddTeamMember(teamId, role, providerId, managerTabId, model)
          : undefined}
        closing={closingTeamIds?.has(teamId)}
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
      onRename={(title) => onRenameTab(activeTab.id, title)}
      orchestrators={orchestrators}
      touchMode={touchMode}
      uiFontScale={uiFontScale}
      activity={activity}
    />
  );
}
