import { useMemo } from "react";
import type { ApiConfig, Workspace } from "../../lib/types";
import type { JobsOpenScope } from "../Jobs/jobsLayout";
import type { JobsTarget } from "../Jobs/JobsView";
import { GeneralInventory } from "./GeneralInventory";
import { GeneralWorkspaceTabs } from "./GeneralWorkspaceTabs";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { SpinalCordButton } from "./SpinalCord";
import {
  useWorkspaceTabActions,
  type SidebarWorkspaceDetailActions,
  type SidebarWorkspaceDetailData,
} from "./SidebarWorkspaceDetail";
import { isGeneralSectionTab, isSidebarTabVisible, tabsInWorkspace } from "./workspaceTabs";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";

/** The protected General workspace: its agents and the machine-wide inventory below them. */
export function SidebarGeneralSection({ workspace, expanded, onToggle, providerId, api, data, actions, workspaces, onOpenJobs }: {
  workspace: Workspace;
  expanded: boolean;
  onToggle: () => void;
  providerId?: string;
  api?: ApiConfig;
  data: SidebarWorkspaceDetailData;
  actions: SidebarWorkspaceDetailActions;
  workspaces: Workspace[];
  onOpenJobs?: (workspaceId: string, target?: JobsTarget, scope?: JobsOpenScope) => void;
}) {
  const workspaceId = workspace.id;
  const sortedTabs = useMemo(() => tabsInWorkspace(workspace.tabs, data.tabs), [workspace.tabs, data.tabs]);
  const generalCwd = workspace.cwd ?? sortedTabs.find((tab) => tab.protected)?.cwd ?? null;
  const isActiveVisual = workspaceId === data.activeWorkspaceId && data.isWorkspaceMode;
  const { activate, activateTab, closeTab, renameTab, openRemoteTab, openSentinelJob, closeTeam } =
    useWorkspaceTabActions(workspaceId, generalCwd, actions);
  const inventoryRowState = useMemo(() => ({
    tabStatuses: data.tabStatuses,
    tabAttentionByTab: data.attention,
    planIndex: data.planIndex,
    closingTabIds: data.closingTabIds,
    appearingTabIds: data.appearingTabIds,
    orchestrators: data.orchestrators,
    uiFontScale: data.uiFontScale,
  }), [data]);

  return (
    <>
    <section data-testid="general-section" className="sidebar-general-section" role="region" aria-label={`${workspace.name} workspace contents`}>
      <div className="sidebar-structural-header sidebar-general-header w-full">
        <SidebarDisclosure data-testid="general-section-toggle" className="sidebar-structural-toggle min-w-0" expanded={expanded} onClick={onToggle}>
          <span className="sidebar-structural-label">GENERAL</span>
        </SidebarDisclosure>
        <SpinalCordButton api={api} />
      </div>
      {expanded && <GeneralWorkspaceTabs
        workspace={workspace}
        providerId={providerId}
        generalCwd={generalCwd}
        activeTabId={data.activeTabIds[workspaceId] ?? null}
        sortedTabs={sortedTabs.filter(isGeneralSectionTab)}
        tabs={data.tabs}
        tabStatuses={data.tabStatuses}
        tabAttentionByTab={data.attention}
        teams={data.teams}
        planIndex={data.planIndex}
        plans={data.plans}
        providers={data.providers}
        providersLoading={data.providersLoading}
        closingTabIds={data.closingTabIds}
        appearingTabIds={data.appearingTabIds}
        isActiveVisual={isActiveVisual}
        workspaceCanvasActive={data.workspaceCanvasActive}
        touchMode={data.touchMode}
        uiFontScale={data.uiFontScale}
        compact={false}
        sidebarActionText={data.sidebarActionText}
        orchestrators={data.orchestrators}
        onBeforeOpenPlan={activate}
        onActivateTab={activateTab}
        onCloseTab={closeTab}
        onRenameTab={renameTab}
        onOpenRemoteTab={openRemoteTab}
        newSessionDisabled={data.newSessionDisabled}
        onRenameTeam={actions.renameTeam}
        onRenamePlan={actions.renamePlan}
        onAddTeamMember={actions.addTeamMember}
        closingTeamIds={data.closingTeamIds}
        onCloseTeam={closeTeam}
        onOpenSentinelJob={openSentinelJob}
      />}
      {expanded && <GeneralInventory
        generalWorkspaceId={workspaceId}
        onShowAllJobs={() => onOpenJobs?.(workspaceId, undefined, "machine")}
        workspaces={workspaces}
        teams={data.teams}
        tabs={data.tabs}
        touchMode={data.touchMode}
        rowState={inventoryRowState}
        onCloseTab={actions.closeTab}
        onRenameTab={actions.renameTab}
        onNavigate={actions.activateTab}
      />}
    </section>
    {expanded && sortedTabs.some((tab) => !isGeneralSectionTab(tab) && isSidebarTabVisible(tab)) && <section aria-label="Other sessions">
      <SidebarWorkspaceTabs workspace={workspace} activeTabId={data.activeTabIds[workspaceId] ?? null}
        sortedTabs={sortedTabs.filter((tab) => !isGeneralSectionTab(tab))} tabs={data.tabs}
        tabStatuses={data.tabStatuses} teams={data.teams} planIndex={data.planIndex} plans={data.plans}
        providers={data.providers} closingTabIds={data.closingTabIds} appearingTabIds={data.appearingTabIds}
        isActiveVisual={isActiveVisual} workspaceCanvasActive={data.workspaceCanvasActive} touchMode={data.touchMode}
        compact={false} sidebarActionText={data.sidebarActionText} orchestrators={data.orchestrators}
        onBeforeOpenPlan={activate} onActivateTab={activateTab} onCloseTab={closeTab} onRenameTab={renameTab} />
    </section>}
    </>
  );
}
