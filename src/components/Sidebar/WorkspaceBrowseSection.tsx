import { Fragment } from "react";
import { usePeerPlanSummaries } from "../Canvas/PlansProvider";
import { remoteViewerIdentity } from "../../lib/remoteTabPortal";
import { surfaceOwnerTitle } from "../../lib/surfaceTab";
import type { Tab } from "../../lib/types";
import type { BrowsedWorkspace } from "../../store/multiplexer/pairedWorkspaceProjection";
import { usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import { TabItem } from "./TabItem";
import { TeamUnit } from "./TeamUnit";
import { remoteTeamIdentity, remoteTeamName } from "./remoteOwnership";
import { selectRemoteTeamPlanProjection } from "./teamTaskStatus";
import { partitionTeamMembers, type TeamTabMember } from "./workspaceTeamModel";
import { countWorkingTabs } from "./tabStatuses";
import { planSidePanelStore } from "../Canvas/planSidePanelStore";
import { openWorkspacePlanCanvas } from "../Canvas/workspacePlanEntry";
import { browsedWorkspaceRows } from "./browsedWorkspaceRows";
import { SurfaceOwnerBlock } from "./SurfaceAttachments";
import { attachmentInset, ownerRowInset, type OwnerRowContext } from "./surfaceAttachmentModel";

interface WorkspaceBrowseSectionProps {
  workspace: BrowsedWorkspace;
  selectedViewerId: string | null;
  attachedTabs: Readonly<Record<string, Tab>>;
  touchMode: boolean;
  onSelectViewer: (viewerId: string) => void;
  onRenameViewer: (tab: Tab, title: string) => void;
  onOpenCanvas?: () => void;
}

export function WorkspaceBrowseSection({
  workspace: { workspace, peerId, teams, tabs, planTitles },
  selectedViewerId, attachedTabs, touchMode, onSelectViewer, onRenameViewer, onOpenCanvas,
}: WorkspaceBrowseSectionProps) {
  const { generals, peerWorkspaces } = usePairedWorkspaceInventory();
  const machines = new Map(generals.map(machine => [machine.peerId, machine]));
  const viewedMachine = machines.get(peerId);
  const remotePlans = usePeerPlanSummaries(peerId, {
    enabled: !viewedMachine || viewedMachine.availability === "online",
  });
  const verifiedViewers = new Set([
    ...generals.filter(machine => !machine.cached).flatMap(machine => machine.sessions.map(session => session.viewerId)),
    ...(peerWorkspaces ?? []).flatMap(source => source.tabs.filter(session => !session.peerId).map(session => session.viewerId)),
  ]);
  const { byViewer, rowsById, drag, attachmentsByOwner, membersByTeam } = browsedWorkspaceRows(tabs);
  const statuses = Object.fromEntries([...byViewer].flatMap(([id, session]) => session.status ? [[id, session.status]] : []));
  const selected = selectedViewerId && byViewer.has(selectedViewerId)
    ? selectedViewerId : tabs.find(({ tab }) => tab.id === workspace.active_tab_id)?.viewerId;
  const row = (tab: Tab, index: number, context: OwnerRowContext, teamName?: string, attachment = false) => {
    const session = byViewer.get(tab.id);
    if (!session) return null;
    const physicalPeer = session.peerId ?? peerId;
    const machine = machines.get(physicalPeer);
    const candidate = attachedTabs[tab.id];
    const attached = candidate && remoteViewerIdentity(candidate)?.peerId === physicalPeer ? candidate : null;
    const canOpen = Boolean(attached) || (machine?.availability === "online" && (!session.peerId || verifiedViewers.has(tab.id)));
    const displayTab = attached ? { ...tab, remote_peer_id: attached.remote_peer_id, remote_tab_id: attached.remote_tab_id,
      remote_peer_name: attached.remote_peer_name } : tab;
    return <TabItem key={tab.id} tab={displayTab} index={index}
      isActive={selected === tab.id} onClick={() => onSelectViewer(tab.id)} onClose={() => undefined}
      onRename={attached ? title => onRenameViewer(attached, title) : undefined}
      canClose={false} readOnly={!attached} workspaceId={workspace.id} hasAgent={Boolean(tab.agent_type)}
      ownerTitle={surfaceOwnerTitle(tab, rowsById)} variant={attachment ? "attachment" : undefined}
      insetPx={attachment ? attachmentInset(ownerRowInset(context, touchMode)) : undefined}
      status={session.status ?? null} touchMode={touchMode}
      observedMachine={{ peerId: physicalPeer, name: machine?.name ?? "Machine", availability: machine?.availability ?? "offline", cached: !canOpen, teamName }} />;
  };
  const withSurfaces = ({ tab, index }: TeamTabMember, context: OwnerRowContext, name?: string) => (
    <SurfaceOwnerBlock key={tab.id} ownerId={tab.id} attachments={attachmentsByOwner.get(tab.id) ?? []} compact={false}
      renderOwner={() => row(tab, index, context, name)}
      renderAttachment={(surface, index) => row(surface, index, context, name, true)} />
  );
  const looseTree = (tab: Tab, index: number, rendered: ReadonlySet<string>) => {
    const visited = new Set([...rendered, tab.id]);
    return <div className="sidebar-tab-tree-row" key={tab.id}>
      {withSurfaces({ tab, index }, "loose")}
      {drag.looseTabs.filter(child => child.parent_tab_id === tab.id && !visited.has(child.id))
        .map((child, index) => looseTree(child, index, visited))}
    </div>;
  };
  return <>
    {drag.units.map(({ key, kind }, index) => {
      if (kind === "tab") return looseTree(rowsById[key], index, new Set());
      const members = (membersByTeam.get(key) ?? []).filter(({ tab }) => !drag.attachedIds.has(tab.id));
      if (!members.length) return null;
      const team = teams.find(candidate => candidate.id === key);
      const foreign = remoteTeamIdentity(members[0].tab, generals);
      const hostPeer = foreign ? members[0].tab.ownership?.peer_id ?? peerId : peerId;
      const host = machines.get(hostPeer);
      const remoteProjection = selectRemoteTeamPlanProjection({
        teamId: team?.id,
        owningPeerId: hostPeer,
        viewedPeerId: peerId,
        plans: remotePlans.plans,
        available: remotePlans.available,
        loading: remotePlans.loading,
        reconnecting: remotePlans.reconnecting,
        error: remotePlans.error,
      });
      const name = remoteTeamName(key, team?.name, foreign?.title, remoteProjection.plan?.title, planTitles?.get(key), workspace.name);
      const { managers, workers, agents, terminals } = partitionTeamMembers(members);
      const section = (items: readonly TeamTabMember[]) => items.map(member => withSurfaces(member, "team", name));
      const working = (items: readonly TeamTabMember[]) => countWorkingTabs(items.map(member => member.tab), statuses);
      return <Fragment key={key}>
        <TeamUnit team={{
          ...team,
          id: JSON.stringify([peerId, workspace.id, key]),
          name: name ?? null,
          lifecycle: team?.lifecycle ?? "forming",
          created_at: team?.created_at ?? 0,
          workspace_id: workspace.id,
          remote_peer_id: hostPeer,
          remote_peer_name: host?.name ?? foreign?.peerName,
        }} workspaceName={workspace.name} plan={remoteProjection.plan} planCountMeta={remoteProjection.meta} tabs={rowsById}
          onOpenEffort={team ? ({ jump, planId }) => {
            const remoteTeam = { peerId, teamId: team.id, name: name ?? workspace.name };
            if (jump) {
              planSidePanelStore.close();
              onOpenCanvas?.();
              openWorkspacePlanCanvas({ remoteTeam, planId, jump });
            } else {
              planSidePanelStore.open({ remoteTeam, planId: planId ?? "" });
            }
          } : undefined}
          managers={section(managers)} managerCount={managers.length} workingManagerCount={working(managers)}
          workers={section(workers)} workerCount={workers.length} workingWorkerCount={working(workers)}
          agents={section(agents)} agentCount={agents.length} workingAgentCount={working(agents)}
          terminals={section(terminals)} terminalCount={terminals.length}
          workingTerminalCount={working(terminals)} providers={[]} />
      </Fragment>;
    })}
  </>;
}
