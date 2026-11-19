import { isFirstRunGeneralTab } from "../../components/AgentPicker/constants";
import { isGeneralAgent } from "../../lib/roleKind";
import { remoteViewerIdentity } from "../../lib/remoteTabPortal";
import { parseCompositeTabId } from "../../lib/remoteTabTransport";
import { relayPeerUsable } from "../../lib/webApi";
import type { MeshPeersResponse, MultiplexerState, PairedPeer, Tab, TabStatus, Team, Workspace } from "../../lib/types";

export type GeneralAvailability = "online" | "reconnecting" | "unreachable" | "offline" | "signed-out" | "update-required";

export interface ObservedPeerTab {
  viewerId: string;
  tab: Tab;
  status?: TabStatus;
  peerId?: string;
}

export interface PairedGeneral {
  peerId: string;
  name: string;
  availability: GeneralAvailability;
  sessions: ObservedPeerTab[];
  teamNames?: ReadonlyMap<string, string>;
  cached?: boolean;
}

export interface BrowsedWorkspace {
  peerId: string;
  workspace: Workspace;
  teams: Team[];
  tabs: ObservedPeerTab[];
  planTitles?: ReadonlyMap<string, string>;
}

export function observedGeneralViewers(
  peers: readonly PairedGeneral[],
  tabs: Readonly<Record<string, Tab>>,
  generalWorkspaceId: string,
): Array<{ peer: PairedGeneral; session: ObservedPeerTab; attached: Tab | null }> {
  const seen = new Set<string>();
  const observed = peers.flatMap(peer => peer.sessions.flatMap(session => {
    if (seen.has(session.viewerId)) return [];
    seen.add(session.viewerId);
    const candidate = tabs[session.viewerId];
    const identity = candidate && remoteViewerIdentity(candidate);
    const attached = identity?.peerId === peer.peerId && identity?.remoteTabId === session.tab.id ? candidate : null;
    if (attached?.workspace_id && attached.workspace_id !== generalWorkspaceId) return [];
    return [{ peer, session, attached }];
  }));
  const byPeerId = new Map(peers.map(peer => [peer.peerId, peer]));
  for (const tab of Object.values(tabs)) {
    if (seen.has(tab.id) || tab.workspace_id !== generalWorkspaceId || !isGeneralAgent(tab)
      || isFirstRunGeneralTab(tab) || tab.hidden || tab.show_in_tabbar === false) continue;
    const identity = remoteViewerIdentity(tab);
    const peer = identity && byPeerId.get(identity.peerId);
    if (peer) observed.push({ peer, session: { viewerId: tab.id, tab }, attached: tab });
  }
  return observed;
}

export function pairedAvailability(peer: PairedPeer, installations: MeshPeersResponse["installations"]): GeneralAvailability {
  if (installations?.some(row => row.active && row.signed_out && row.instance_id === peer.instance_id)) return "signed-out";
  if (!peer.online) return "offline";
  return relayPeerUsable(peer) ? "reconnecting" : "update-required";
}

function localSessions(peerId: string, snapshot: MultiplexerState, workspace: Workspace): ObservedPeerTab[] {
  return [...new Set(workspace.tabs)].flatMap(id => {
    const tab = snapshot.tabs[id];
    const viewerId = `mesh:${peerId}/${id}`;
    if (!parseCompositeTabId(viewerId) || id.startsWith("mesh:") || tab?.id !== id
      || tab.remote_peer_id || tab.remote_tab_id || (tab.workspace_id && tab.workspace_id !== workspace.id)
      || tab.hidden || tab.show_in_tabbar === false) return [];
    return [{ viewerId, tab, status: snapshot.tab_statuses?.[id] }];
  });
}

export function projectPairedGeneral(peerId: string, snapshot: MultiplexerState): ObservedPeerTab[] {
  const general = snapshot.workspaces.find(workspace => workspace.protected);
  return general ? localSessions(peerId, snapshot, general).filter(({ tab }) => isGeneralAgent(tab) && !isFirstRunGeneralTab(tab)) : [];
}

export function projectBrowsedWorkspaces(peerId: string, snapshot: MultiplexerState): BrowsedWorkspace[] {
  return snapshot.workspaces.filter(workspace => !workspace.protected).map(workspace => ({
    peerId,
    workspace,
    teams: (snapshot.teams ?? []).filter(team => team.workspace_id === workspace.id)
      .map(team => ({ ...team, remote_peer_id: peerId })),
    tabs: [...localSessions(peerId, snapshot, workspace), ...workspace.tabs.flatMap(id => {
      const tab = snapshot.tabs[id];
      const route = parseCompositeTabId(id);
      if (!route || !tab || tab.id !== id || tab.workspace_id !== workspace.id || tab.hidden
        || tab.show_in_tabbar === false || tab.remote_peer_id !== route.peerId || tab.remote_tab_id !== route.remoteTabId) return [];
      return [{ viewerId: id, tab, peerId: route.peerId, status: snapshot.tab_statuses?.[id] }];
    })],
  }));
}
