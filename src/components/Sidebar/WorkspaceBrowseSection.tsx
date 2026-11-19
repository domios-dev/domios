import { useMeshPeerName } from "../../lib/useMeshMachineIdentities";
import { useState } from "react";
import { cleanMachineName } from "../../lib/machineIdentity";
import type { Tab } from "../../lib/types";
import { usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import type { BrowsedWorkspace, ObservedPeerTab } from "../../store/multiplexer/pairedWorkspaceProjection";
import { TabItem } from "./TabItem";
import { TeamRunMachine } from "./RemoteTabPresentation";
import { remoteTeamName } from "./remoteOwnership";
import { SidebarDisclosure } from "./SidebarDisclosure";

interface WorkspaceBrowseSectionProps {
  selectedViewerId: string | null;
  attachedTabs: Readonly<Record<string, Tab>>;
  touchMode: boolean;
  onSelectViewer: (viewerId: string) => void;
  onRenameViewer: (tab: Tab, title: string) => void;
}

export function WorkspaceBrowseSection(props: WorkspaceBrowseSectionProps) {
  const { browse } = usePairedWorkspaceInventory();
  if (!browse || browse.availability !== "online") return null;
  const machine = useMeshPeerName(browse.target.id) ?? cleanMachineName(browse.target.name, browse.target.id);
  return browse.workspaces.filter(workspace => workspace.tabs.length > 0).map(workspace => <WorkspaceRows
    key={JSON.stringify([workspace.peerId, workspace.workspace.id])}
    workspace={workspace}
    hostName={machine}
    {...props}
  />);
}

function WorkspaceRows({ workspace: { workspace, peerId, teams, tabs }, hostName, selectedViewerId, attachedTabs, touchMode, onSelectViewer, onRenameViewer }: WorkspaceBrowseSectionProps & {
  workspace: BrowsedWorkspace;
  hostName?: string;
}) {
  const [expanded, setExpanded] = useState(true);
  const teamIds = new Set(teams.map(team => team.id));
  const row = ({ viewerId, tab, status }: ObservedPeerTab, index: number) => {
    const attached = attachedTabs[viewerId];
    return <TabItem key={viewerId} tab={attached ?? { ...tab, id: viewerId }} index={index}
      isActive={selectedViewerId === viewerId} onClick={() => onSelectViewer(viewerId)} onClose={() => undefined}
      onRename={attached ? title => onRenameViewer(attached, title) : undefined}
      canClose={false} readOnly={!attached} workspaceId={workspace.id} hasAgent={Boolean(tab.agent_type)}
      status={status ?? null} touchMode={touchMode}
      observedMachine={{ peerId, name: hostName ?? "Machine", availability: "online" }} />;
  };
  return <section aria-label={workspace.name} className="px-2">
    <SidebarDisclosure expanded={expanded} onToggle={() => setExpanded(value => !value)}>
      <span className="truncate">{workspace.name}</span>
    </SidebarDisclosure>
    <div hidden={!expanded}>
      {tabs.filter(({ tab }) => !tab.team_id || !teamIds.has(tab.team_id)).map(row)}
      {teams.map(team => {
        const members = tabs.filter(({ tab }) => tab.team_id === team.id);
        if (!members.length) return null;
        const name = remoteTeamName(team.id, team.name) ?? "Team";
        return <div key={team.id}>
          <TeamRunMachine peerId={peerId} fallbackName={hostName} />
          <section aria-label={name} className="sidebar-team-unit">
            <div className="sidebar-team-header"><span className="sidebar-team-name">{name}</span></div>
            {members.map(row)}
          </section>
        </div>;
      })}
    </div>
  </section>;
}
