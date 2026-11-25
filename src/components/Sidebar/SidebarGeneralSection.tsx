import { isConciergeTab } from "../../lib/tabInputPolicy";
import { useMemo } from "react";
import type { ApiConfig, Workspace } from "../../lib/types";
import type { JobsOpenScope } from "../Jobs/jobsLayout";
import type { JobsTarget } from "../Jobs/JobsView";
import { GeneralInventory } from "./GeneralInventory";
import { useNeighborGatewayIds } from "../Neighbors/neighborGateways";
import { GeneralWorkspaceTabs } from "./GeneralWorkspaceTabs";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { SpinalCordButton } from "./SpinalCord";
import {
  useWorkspaceTabActions,
  type SidebarWorkspaceDetailActions,
  type SidebarWorkspaceDetailData,
} from "./SidebarWorkspaceDetail";
import { isGeneralSectionTab, isSidebarTabVisible, tabsInWorkspace } from "./workspaceTabs";
import { isWorkspaceTabActive } from "./sidebarSelection";
import { useAppearingTabs } from "./useAppearingTabs";
import { projectUtilityShells } from "../General/generalOverviewModel";
import { isSurfaceTab, surfaceOwnerId } from "../../lib/surfaceTab";
import { usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import { GeneralListMachineContext, TabItem } from "./TabItem";
import { localInstanceLabel } from "../../lib/localInstanceName";
import { useAccountMachineNames, useMeshMachineIdentities } from "../../lib/useMeshMachineIdentities";
import { observedGeneralViewers } from "../../store/multiplexer/pairedWorkspaceProjection";
import { useSidebarMachineFilter } from "./sidebarMachineFilter";
import { remoteViewerIdentity } from "../../lib/remoteTabPortal";

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
  const { generals } = usePairedWorkspaceInventory();
  const machineNames = useAccountMachineNames();
  const { local_instance_name } = useMeshMachineIdentities();
  const localMachine = machineNames.local ?? local_instance_name ?? localInstanceLabel();
  const machineFilter = useSidebarMachineFilter();
  const pairedGenerals = useMemo(() => [...generals].sort((a, b) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true }) || a.peerId.localeCompare(b.peerId)), [generals]);
  const viewers = useMemo(() => observedGeneralViewers(pairedGenerals, data.tabs, workspace.id), [pairedGenerals, data.tabs, workspace.id]);
  const workspaceId = workspace.id;
  const sortedTabs = useMemo(() => tabsInWorkspace(workspace.tabs, data.tabs), [workspace.tabs, data.tabs]);
  const conciergeIds = useNeighborGatewayIds();
  const sessionTabs = useMemo(() => {
    const utilityIds = new Set(projectUtilityShells(workspaces, data.tabs, data.teams, workspaceId).map(({ tab }) => tab.id));
    const sessions = sortedTabs.filter((tab) => !utilityIds.has(tab.id)
      && (tab.team_id || !remoteViewerIdentity(tab))
      && !(isSurfaceTab(tab) && utilityIds.has(surfaceOwnerId(tab) ?? "")) && !isConciergeTab(tab) && !conciergeIds.has(tab.id));
    return [...sessions.filter(isGeneralSectionTab), ...sessions.filter((tab) => !isGeneralSectionTab(tab))];
  }, [sortedTabs, workspaces, data.tabs, data.teams, workspaceId, conciergeIds]);
  const generalCwd = workspace.cwd ?? sortedTabs.find((tab) => tab.protected)?.cwd ?? null;
  const isActiveVisual = workspaceId === data.activeWorkspaceId && data.isWorkspaceMode;
  const { activate, activateTab, closeTab, renameTab, openSentinelJob, closeTeam } =
    useWorkspaceTabActions(workspaceId, generalCwd, actions);
  // The inventory lists shells of every workspace, so its entrances follow all of them.
  const machineTabIds = useMemo(() => workspaces.flatMap((candidate) => candidate.tabs), [workspaces]);
  const appearingTabIds = useAppearingTabs(machineTabIds);
  const inventoryRowState = useMemo(() => ({
    tabStatuses: data.tabStatuses,
    tabAttentionByTab: data.attention,
    planIndex: data.planIndex,
    closingTabIds: data.closingTabIds,
    appearingTabIds,
    orchestrators: data.orchestrators,
    uiFontScale: data.uiFontScale,
  }), [data, appearingTabIds]);

  if (typeof machineFilter === "object" && !viewers.some(({ peer }) => peer.peerId === machineFilter.peerId)
    && !sessionTabs.some(tab => tab.remote_peer_id === machineFilter.peerId && isSidebarTabVisible(tab))) return null;

  return (
    <section data-testid="general-section" className="sidebar-general-section" role="region" aria-label={`${workspace.name} workspace contents`}>
      <div className="sidebar-structural-header sidebar-general-header w-full">
        <SidebarDisclosure data-testid="general-section-toggle" className="sidebar-structural-toggle min-w-0" expanded={expanded} onClick={onToggle}>
          <span className="sidebar-structural-label">GENERAL</span>
        </SidebarDisclosure>
        <SpinalCordButton api={api} />
      </div>
      {expanded && <GeneralListMachineContext.Provider value={localMachine}><GeneralWorkspaceTabs
        workspace={workspace}
        providerId={providerId}
        generalCwd={generalCwd}
        observedRows={viewers.map(({ peer, session, attached }, index) => (
          <TabItem
            key={session.viewerId}
            tab={attached ?? { ...session.tab, id: session.viewerId }}
            index={index}
            isActive={isWorkspaceTabActive(isActiveVisual, data.workspaceCanvasActive, session.viewerId, data.activeTabIds[workspaceId])}
            onClick={() => activateTab(session.viewerId)}
            onClose={() => closeTab(session.viewerId)}
            onRename={attached ? title => renameTab(session.viewerId, title) : undefined}
            canClose={!session.tab.protected}
            readOnly={!attached}
            workspaceId={workspaceId}
            hasAgent={Boolean(session.tab.agent_type)}
            status={data.tabStatuses[session.viewerId] ?? session.status ?? null}
            attention={attached ? data.attention[session.viewerId] : null}
            observedMachine={{ peerId: peer.peerId, name: peer.name, availability: peer.availability, cached: peer.cached && !attached }}
          />
        ))}
        activeTabId={data.activeTabIds[workspaceId] ?? null}
        sortedTabs={sessionTabs}
        tabs={data.tabs}
        tabStatuses={data.tabStatuses}
        tabAttentionByTab={data.attention}
        teams={data.teams}
        planIndex={data.planIndex}
        plans={data.plans}
        providers={data.providers}
        providersLoading={data.providersLoading}
        closingTabIds={data.closingTabIds}
        appearingTabIds={appearingTabIds}
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
        newSessionDisabled={data.newSessionDisabled}
        onRenameTeam={actions.renameTeam}
        onRenamePlan={actions.renamePlan}
        onAddTeamMember={actions.addTeamMember}
        closingTeamIds={data.closingTeamIds}
        onCloseTeam={closeTeam}
        onOpenSentinelJob={openSentinelJob}
      /></GeneralListMachineContext.Provider>}
      {expanded && <div hidden={typeof machineFilter === "object"}><GeneralInventory
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
      /></div>}
    </section>
  );
}
