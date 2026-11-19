import { useState } from "react";
import { usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import type { BrowsedWorkspace, ObservedPeerTab } from "../../store/multiplexer/pairedWorkspaceProjection";
import { MachineProvenance } from "../shared/MachineProvenance";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { StatusDot } from "./StatusDot";

interface WorkspaceBrowseSectionProps {
  selectedViewerId: string | null;
  onSelectViewer: (viewerId: string) => void;
}

export function WorkspaceBrowseSection({ selectedViewerId, onSelectViewer }: WorkspaceBrowseSectionProps) {
  const { browse } = usePairedWorkspaceInventory();
  if (!browse) return null;
  if (browse.availability !== "online") {
    const status = browse.availability === "reconnecting" ? "Connecting"
      : browse.availability.replace("-", " ");
    return <p role="status" className="px-3 py-2 text-xs text-[var(--ct-muted-fg)]">{browse.target.name} · {status}</p>;
  }
  if (!browse.workspaces.length) return <p className="px-3 py-2 text-xs text-[var(--ct-muted-fg)]">No workspaces</p>;
  return browse.workspaces.map(workspace => <WorkspaceRows
    key={JSON.stringify([workspace.peerId, workspace.workspace.id])}
    workspace={workspace}
    hostName={browse.target.name}
    selectedViewerId={selectedViewerId}
    onSelectViewer={onSelectViewer}
  />);
}

function WorkspaceRows({ workspace: { workspace, teams, tabs }, hostName, selectedViewerId, onSelectViewer }: WorkspaceBrowseSectionProps & {
  workspace: BrowsedWorkspace;
  hostName: string;
}) {
  const [expanded, setExpanded] = useState(true);
  const teamIds = new Set(teams.map(team => team.id));
  const row = ({ viewerId, tab, status }: ObservedPeerTab) => (
    <button key={viewerId} type="button" aria-current={selectedViewerId === viewerId || undefined}
      className="ct-menu-row min-h-[44px] sm:min-h-0" onClick={() => onSelectViewer(viewerId)}>
      <StatusDot status={status ?? null} />
      <span className="truncate">{tab.title}</span>
    </button>
  );
  return <section aria-label={workspace.name} className="px-2">
    <SidebarDisclosure expanded={expanded} onToggle={() => setExpanded(value => !value)}>
      <span className="truncate">{workspace.name}</span>
    </SidebarDisclosure>
    {expanded && <>
      {tabs.filter(({ tab }) => !tab.team_id || !teamIds.has(tab.team_id)).map(row)}
      {teams.map(team => <div key={team.id} role="group" aria-label={team.name ?? "Team"} className="pl-2">
        <div className="px-2 py-1 text-xs">
          {team.name ?? "Team"}
          <MachineProvenance peerId={team.remote_peer_id} name={hostName} />
        </div>
        {tabs.filter(({ tab }) => tab.team_id === team.id).map(row)}
      </div>)}
    </>}
  </section>;
}
