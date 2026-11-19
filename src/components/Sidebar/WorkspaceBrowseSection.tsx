import { isConciergeTab } from "../../lib/tabInputPolicy";
import type { Tab } from "../../lib/types";
import type { BrowsedWorkspace } from "../../store/multiplexer/pairedWorkspaceProjection";
import { usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import { TabItem } from "./TabItem";
import { TeamUnit } from "./TeamUnit";
import { remoteTeamName } from "./remoteOwnership";
import { SidebarWholeTeam, useSidebarMachineFilter, wholeSidebarTeam } from "./sidebarMachineFilter";
import { partitionTeamMembers } from "./workspaceTeamModel";
import { countWorkingTabs } from "./tabStatuses";

interface WorkspaceBrowseSectionProps {
  workspace: BrowsedWorkspace;
  selectedViewerId: string | null;
  attachedTabs: Readonly<Record<string, Tab>>;
  touchMode: boolean;
  onSelectViewer: (viewerId: string) => void;
  onRenameViewer: (tab: Tab, title: string) => void;
}

export function WorkspaceBrowseSection({
  workspace: { workspace, peerId, teams, tabs, planTitles },
  selectedViewerId, attachedTabs, touchMode, onSelectViewer, onRenameViewer,
}: WorkspaceBrowseSectionProps) {
  const filter = useSidebarMachineFilter();
  const { generals, peerWorkspaces } = usePairedWorkspaceInventory();
  const machines = new Map(generals.map(machine => [machine.peerId, machine]));
  const verifiedViewers = new Set([
    ...generals.filter(machine => !machine.cached).flatMap(machine => machine.sessions.map(session => session.viewerId)),
    ...(peerWorkspaces ?? []).flatMap(source => source.tabs.filter(session => !session.peerId).map(session => session.viewerId)),
  ]);
  const byViewer = new Map(tabs.filter(({ tab }) => !isConciergeTab(tab)).map(session => [session.viewerId, session] as const));
  const members = [...byViewer.values()].map(({ viewerId, tab }, index) => ({ tab: { ...tab, id: viewerId }, index }));
  const statuses = Object.fromEntries([...byViewer].flatMap(([id, session]) => session.status ? [[id, session.status]] : []));
  const teamIds = new Set(members.flatMap(({ tab }) => tab.team_id ? [tab.team_id] : []));
  const row = ({ tab, index }: typeof members[number], teamName?: string) => {
    const session = byViewer.get(tab.id);
    if (!session) return null;
    const physicalPeer = session.peerId ?? peerId;
    const machine = machines.get(physicalPeer);
    const attached = attachedTabs[tab.id];
    const selected = selectedViewerId === tab.id;
    const canOpen = Boolean(attached) || (machine?.availability === "online" && (!session.peerId || verifiedViewers.has(tab.id)));
    return <TabItem key={tab.id} tab={attached ?? tab} index={index}
      isActive={selected} onClick={() => onSelectViewer(tab.id)} onClose={() => undefined}
      onRename={attached ? title => onRenameViewer(attached, title) : undefined}
      canClose={false} readOnly={!attached} workspaceId={workspace.id} hasAgent={Boolean(tab.agent_type)}
      status={session.status ?? null} touchMode={touchMode}
      observedMachine={{ peerId: physicalPeer, name: machine?.name ?? "Machine", availability: machine?.availability ?? "offline", cached: !canOpen, teamName }} />;
  };
  return <>
    {members.filter(({ tab }) => !tab.team_id).map(member => row(member))}
    {[...teamIds].map(teamId => {
      const team = teams.find(candidate => candidate.id === teamId);
      const teamMembers = wholeSidebarTeam(filter, members.filter(({ tab }) => tab.team_id === teamId),
        ({ tab }) => byViewer.get(tab.id)?.peerId ?? peerId);
      if (!teamMembers.length) return null;
      const { managers, workers, agents, terminals, surfacesByOwner } = partitionTeamMembers(teamMembers);
      const name = remoteTeamName(teamId, team?.name, planTitles?.get(teamId), workspace.name);
      const section = (items: typeof members) => items.flatMap(member => [
        row(member, name), ...(surfacesByOwner.get(member.tab.id) ?? []).map(surface => row(surface, name)),
      ]);
      const host = machines.get(peerId);
      const working = (items: typeof members) => countWorkingTabs(items.map(member => member.tab), statuses);
      return <SidebarWholeTeam.Provider key={teamId} value>
        <TeamUnit team={{
          ...team,
          id: JSON.stringify([peerId, teamId]),
          name: name ?? null,
          lifecycle: team?.lifecycle ?? "forming",
          created_at: team?.created_at ?? 0,
          workspace_id: workspace.id,
          remote_peer_id: peerId,
          remote_peer_name: host?.name,
        }} workspaceName={workspace.name} plan={null} tabs={attachedTabs}
          managers={section(managers)} managerCount={managers.length} workingManagerCount={working(managers)}
          workers={section(workers)} workerCount={workers.length} workingWorkerCount={working(workers)}
          agents={section(agents)} agentCount={agents.length} workingAgentCount={working(agents)}
          terminals={terminals.map(member => row(member, name))} terminalCount={terminals.length}
          workingTerminalCount={working(terminals)} providers={[]} />
      </SidebarWholeTeam.Provider>;
    })}
  </>;
}
