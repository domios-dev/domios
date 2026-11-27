import { isFirstRunGeneralTab } from "../../components/AgentPicker/constants";
import { cleanMachineName } from "../../lib/machineIdentity";
import { isGeneralAgent } from "../../lib/roleKind";
import { remoteViewerIdentity } from "../../lib/remoteTabPortal";
import { parseCompositeTabId } from "../../lib/remoteTabTransport";
import { isConciergeTab } from "../../lib/tabInputPolicy";
import { relayPeerUsable } from "../../lib/webApi";
import type { MeshPeersResponse, MultiplexerState, PairedPeer, Tab, TabStatus, Team, Workspace } from "../../lib/types";

export type GeneralAvailability = "online" | "reconnecting" | "unreachable" | "offline" | "signed-out" | "update-required";

export interface ObservedPeerTab {
  viewerId: string;
  tab: Tab;
  status?: TabStatus;
  peerId?: string;
}

export interface PeerLastActiveSelection {
  viewerId: string;
  workspaceId: string | null;
}

export interface PairedGeneral {
  peerId: string;
  name: string;
  instanceName?: string;
  installation?: Pick<NonNullable<MeshPeersResponse["installations"]>[number], "installation_id" | "name">;
  availability: GeneralAvailability;
  sessions: ObservedPeerTab[];
  teamNames?: ReadonlyMap<string, string>;
  lastActiveTab?: PeerLastActiveSelection | null;
  cached?: boolean;
}

export function pairedPeerIdentity(peer: Pick<PairedPeer, "instance_id" | "instance_name">,
  installations: MeshPeersResponse["installations"], displayName?: string) {
  return {
    peerId: peer.instance_id,
    instanceName: peer.instance_name,
    name: displayName ?? cleanMachineName(peer.instance_name, peer.instance_id) ?? "Machine",
    installation: installations?.find(row => row.active && row.instance_id === peer.instance_id),
  };
}

export interface BrowsedWorkspace {
  peerId: string;
  workspace: Workspace;
  teams: Team[];
  tabs: ObservedPeerTab[];
  planTitles?: ReadonlyMap<string, string>;
  selected?: boolean;
  tabIds?: readonly string[];
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
    return [{ viewerId, tab: tab.workspace_id ? tab : { ...tab, workspace_id: workspace.id }, status: snapshot.tab_statuses?.[id] }];
  });
}

export function projectPairedGeneral(peerId: string, snapshot: MultiplexerState): ObservedPeerTab[] {
  const general = snapshot.workspaces.find(workspace => workspace.protected);
  return general ? localSessions(peerId, snapshot, general).filter(({ tab }) => isGeneralAgent(tab) && !isFirstRunGeneralTab(tab)) : [];
}

export function deskWorkspaceMembership(
  workspaces: readonly Workspace[],
  tabs: Readonly<Record<string, Tab>>,
  teams: readonly Team[],
  ordinaryTabs: readonly Tab[] = [],
): Map<string, Tab[]> {
  const byWorkspace = new Map(workspaces.map(workspace => [workspace.id, [] as Tab[]]));
  const teamOwners = new Map(teams.map(team => [team.id, team.workspace_id]));
  const seen = new Set<string>();
  const place = (tab: Tab, workspaceId: string) => {
    const teamOwner = tab.team_id ? teamOwners.get(tab.team_id) : undefined;
    const destination = byWorkspace.get(teamOwner ?? tab.workspace_id ?? workspaceId);
    if (!destination || seen.has(tab.id)) return;
    seen.add(tab.id);
    destination.push(tab);
  };
  for (const workspace of workspaces) {
    for (const id of workspace.tabs) {
      const tab = tabs[id];
      if (tab?.id === id) place(tab, workspace.id);
    }
  }
  for (const tab of ordinaryTabs) {
    if (tab.workspace_id && remoteViewerIdentity(tab)) place(tab, tab.workspace_id);
  }
  for (const members of byWorkspace.values()) members.sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
  return byWorkspace;
}

export function projectBrowsedWorkspaces(
  peerId: string,
  snapshot: MultiplexerState,
  ordinaryTabs: readonly Tab[] = [],
): BrowsedWorkspace[] {
  const ordered = [...snapshot.workspaces].sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
  const teams = snapshot.teams ?? [];
  const membership = deskWorkspaceMembership(ordered, snapshot.tabs, teams, ordinaryTabs);
  return ordered.filter(workspace => !workspace.protected).map(workspace => {
    const members = membership.get(workspace.id) ?? [];
    const sessions = members.flatMap(tab => {
      const route = remoteViewerIdentity(tab);
      const viewerId = route ? tab.id : `mesh:${peerId}/${tab.id}`;
      if ((!route && (tab.id.startsWith("mesh:") || tab.remote_peer_id || tab.remote_tab_id)) || !parseCompositeTabId(viewerId)) return [];
      const ownedTab = !route && !tab.workspace_id ? { ...tab, workspace_id: workspace.id } : tab;
      return [{ viewerId, tab: ownedTab, peerId: route?.peerId, status: snapshot.tab_statuses?.[tab.id] }];
    });
    return {
      peerId,
      workspace,
      selected: snapshot.active_workspace_id === workspace.id,
      teams: teams.filter(team => team.workspace_id === workspace.id).map(team => ({ ...team, remote_peer_id: peerId })),
      tabIds: sessions.map(session => session.viewerId),
      tabs: sessions.filter(({ tab }) => !tab.hidden && tab.show_in_tabbar !== false),
    };
  });
}

export type PeerOrdinaryTabSummary = Tab & { focused?: boolean };

export function resolvePeerLastActiveTab(
  snapshot: MultiplexerState,
  generalSessions: readonly ObservedPeerTab[],
  browsedWorkspaces: readonly BrowsedWorkspace[],
  ordinaryTabs: readonly PeerOrdinaryTabSummary[] = [],
): PeerLastActiveSelection | null {
  const byRawId = new Map<string, PeerLastActiveSelection>();
  for (const session of generalSessions) {
    byRawId.set(session.tab.id, { viewerId: session.viewerId, workspaceId: null });
  }
  for (const workspace of browsedWorkspaces) {
    for (const session of workspace.tabs) {
      if (session.peerId || isConciergeTab(session.tab) || byRawId.has(session.tab.id)) continue;
      byRawId.set(session.tab.id, { viewerId: session.viewerId, workspaceId: workspace.workspace.id });
    }
  }
  const liveFocused = ordinaryTabs.find(tab => tab.focused === true && !tab.remote_peer_id && !tab.id.startsWith("mesh:"));
  if (liveFocused) {
    const match = byRawId.get(liveFocused.id);
    if (match) return match;
  }
  const activeWorkspace = snapshot.active_workspace_id
    ? snapshot.workspaces.find(workspace => workspace.id === snapshot.active_workspace_id)
    : undefined;
  if (activeWorkspace?.active_tab_id) {
    const match = byRawId.get(activeWorkspace.active_tab_id);
    if (match) return match;
  }
  if (snapshot.focused_tab_id) {
    const match = byRawId.get(snapshot.focused_tab_id);
    if (match) return match;
  }
  const main = generalSessions.find(session => session.tab.protected) ?? generalSessions[0];
  return main ? { viewerId: main.viewerId, workspaceId: null } : null;
}
