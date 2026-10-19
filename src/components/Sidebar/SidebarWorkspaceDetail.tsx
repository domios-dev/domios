import { useCallback, useMemo } from "react";
import type { ProviderDescriptor, Tab, TabStatus, Team, Workspace } from "../../lib/types";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Plan } from "../Canvas/types";
import type { ActiveWorkSnapshot } from "../Canvas/planView/activeWork";
import type { JobsTarget } from "../Jobs/JobsView";
import type { TeamMemberRole } from "../../lib/teamActions";
import { WorkspaceItem } from "./WorkspaceItem";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";
import type { TabAttention } from "./tabAttention";
import { tabsInWorkspace } from "./workspaceTabs";

/** Sidebar-wide data every workspace detail reads; built once per sidebar render. */
export interface SidebarWorkspaceDetailData {
  tabs: Record<string, Tab>;
  tabStatuses: Record<string, TabStatus>;
  attention: Readonly<Record<string, TabAttention>>;
  teams: Team[];
  planIndex: PlanIndexEntry[];
  plans: Plan[];
  activeWork: ActiveWorkSnapshot | null;
  providers: ProviderDescriptor[];
  providersLoading: boolean;
  closingTabIds: Set<string>;
  appearingTabIds: Set<string>;
  closingTeamIds: ReadonlySet<string>;
  activeTabIds: Record<string, string>;
  activeWorkspaceId: string | null;
  isWorkspaceMode: boolean;
  workspaceCanvasActive: boolean;
  touchMode: boolean;
  uiFontScale?: number;
  compact: boolean;
  sidebarActionText: string;
  orchestrators: Tab[];
  newSessionDisabled: boolean;
}

/** Sidebar actions, keyed by workspace id so one stable set serves every workspace. */
export interface SidebarWorkspaceDetailActions {
  activateWorkspace: (workspaceId: string) => void;
  activateTab: (workspaceId: string, tabId: string) => void;
  closeTab: (workspaceId: string, tabId: string) => void;
  renameTab: (workspaceId: string, tabId: string, title: string) => void;
  renameWorkspace: (workspaceId: string, name: string) => void;
  closeWorkspace: (workspaceId: string) => void;
  newTerminal: (workspaceId: string, shell?: string) => void;
  newSession: (workspaceId: string, cwd: string | null) => void;
  newAgent: (agentId: string, workspaceId: string, cwd: string | null) => void;
  openRemoteTab: (workspaceId: string) => void;
  openMap?: (workspaceId: string) => void;
  openAutomation?: (workspaceId: string, target?: JobsTarget) => void;
  renamePlan?: (planId: string, title: string) => Promise<void>;
  renameTeam?: (teamId: string, name: string, managerTabId: string | null) => void | Promise<void>;
  addTeamMember?: (teamId: string, role: TeamMemberRole, providerId: string, managerTabId: string | null, model?: string) => void | Promise<void>;
  closeTeam?: (workspaceId: string, teamId: string) => void;
}

/** The sidebar actions bound to one workspace, stable while the workspace and actions are. */
export function useWorkspaceTabActions(workspaceId: string, cwd: string | null, actions: SidebarWorkspaceDetailActions) {
  const activate = useCallback(() => actions.activateWorkspace(workspaceId), [actions, workspaceId]);
  const activateTab = useCallback((tabId: string) => actions.activateTab(workspaceId, tabId), [actions, workspaceId]);
  const closeTab = useCallback((tabId: string) => actions.closeTab(workspaceId, tabId), [actions, workspaceId]);
  const renameTab = useCallback((tabId: string, title: string) => actions.renameTab(workspaceId, tabId, title), [actions, workspaceId]);
  const newSession = useCallback(() => actions.newSession(workspaceId, cwd), [actions, workspaceId, cwd]);
  const openRemoteTab = useCallback(() => actions.openRemoteTab(workspaceId), [actions, workspaceId]);
  const { openAutomation, closeTeam } = actions;
  const openSentinelJob = useMemo(
    () => (openAutomation ? (target: JobsTarget) => openAutomation(workspaceId, target) : undefined),
    [openAutomation, workspaceId],
  );
  const closeTeamInWorkspace = useMemo(
    () => (closeTeam ? (teamId: string) => closeTeam(workspaceId, teamId) : undefined),
    [closeTeam, workspaceId],
  );
  return { activate, activateTab, closeTab, renameTab, newSession, openRemoteTab, openSentinelJob, closeTeam: closeTeamInWorkspace };
}

/** The selected workspace's header and tab tree. */
export function SidebarWorkspaceDetail({ workspace, status, data, actions }: {
  workspace: Workspace;
  status: TabStatus | null;
  data: SidebarWorkspaceDetailData;
  actions: SidebarWorkspaceDetailActions;
}) {
  const workspaceId = workspace.id;
  const cwd = workspace.cwd ?? null;
  const isActiveVisual = workspaceId === data.activeWorkspaceId && data.isWorkspaceMode;
  const sortedTabs = useMemo(() => tabsInWorkspace(workspace.tabs, data.tabs), [workspace.tabs, data.tabs]);
  const { activate, activateTab, closeTab, renameTab, newSession, openRemoteTab, openSentinelJob, closeTeam } =
    useWorkspaceTabActions(workspaceId, cwd, actions);
  const { openMap, openAutomation } = actions;

  return (
    <div data-testid="workspace-detail" data-workspace-id={workspaceId} role="region" aria-label={`${workspace.name} workspace contents`}>
      <WorkspaceItem
        workspace={workspace}
        isActive={isActiveVisual}
        isExpanded
        onClick={activate}
        onToggleExpand={() => {}}
        onClose={() => actions.closeWorkspace(workspaceId)}
        onRename={(name) => actions.renameWorkspace(workspaceId, name)}
        onNewTerminal={(shell) => actions.newTerminal(workspaceId, shell)}
        onNewSession={newSession}
        onOpenMap={() => openMap?.(workspaceId)}
        onOpenAutomation={() => openAutomation?.(workspaceId)}
        newSessionDisabled={data.newSessionDisabled}
        onNewAgent={(agentId) => actions.newAgent(agentId, workspaceId, cwd)}
        canClose={true}
        status={status}
        showDisclosure={false}
        detailHeader
        touchMode={data.touchMode}
        countActiveJobs
        uiFontScale={data.uiFontScale}
      />
      <SidebarWorkspaceTabs
        workspace={workspace}
        activeTabId={data.activeTabIds[workspaceId] ?? null}
        sortedTabs={sortedTabs}
        tabs={data.tabs}
        tabStatuses={data.tabStatuses}
        tabAttentionByTab={data.attention}
        teams={data.teams}
        planIndex={data.planIndex}
        plans={data.plans}
        activeWork={data.activeWork}
        providers={data.providers}
        providersLoading={data.providersLoading}
        closingTabIds={data.closingTabIds}
        appearingTabIds={data.appearingTabIds}
        isActiveVisual={isActiveVisual}
        workspaceCanvasActive={data.workspaceCanvasActive}
        touchMode={data.touchMode}
        uiFontScale={data.uiFontScale}
        compact={data.compact}
        sidebarActionText={data.sidebarActionText}
        orchestrators={data.orchestrators}
        onBeforeOpenPlan={activate}
        onActivateTab={activateTab}
        onCloseTab={closeTab}
        onRenameTab={renameTab}
        onNewSession={newSession}
        onOpenRemoteTab={openRemoteTab}
        newSessionDisabled={data.newSessionDisabled}
        onRenameTeam={actions.renameTeam}
        onRenamePlan={actions.renamePlan}
        onAddTeamMember={actions.addTeamMember}
        closingTeamIds={data.closingTeamIds}
        onCloseTeam={closeTeam}
        onOpenSentinelJob={openSentinelJob}
      />
    </div>
  );
}
