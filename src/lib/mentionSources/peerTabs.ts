import type { BrowsedWorkspace, PairedGeneral } from "../../store/multiplexer/pairedWorkspaceProjection";
import { cleanMachineName } from "../machineIdentity";
import { parseCompositeTabId } from "../remoteTabTransport";
import { projectAttachedRemoteTab } from "../remoteTabPortal";
import type { Tab, TabStatus } from "../types";

export interface MentionPeerInventory {
  generals: readonly PairedGeneral[];
  peerWorkspaces?: readonly BrowsedWorkspace[];
}

export interface PeerMentionTab {
  tab: Tab;
  workspaceId: string;
  workspaceName?: string;
  peerName?: string;
  status?: TabStatus;
}

export function peerMentionTabs(inventory: MentionPeerInventory, peerId: string): Map<string, PeerMentionTab> {
  const peer = inventory.generals.find(row => row.peerId === peerId);
  const result = new Map<string, PeerMentionTab>();
  if (peer?.availability !== "online" || peer.cached) return result;

  const workspaces = (inventory.peerWorkspaces ?? []).filter(row => row.peerId === peerId);
  const records = [
    ...peer.sessions.map(session => ({ session, workspaceId: session.tab.workspace_id })),
    ...workspaces.flatMap(({ workspace, tabs }) => tabs.map(session => ({
      session, workspaceId: workspace.id, workspaceName: workspace.name,
    }))),
  ];
  const peerName = cleanMachineName(peer.name, peerId) ?? peer.name;
  for (const { session, workspaceId, ...placement } of records) {
    const target = parseCompositeTabId(session.viewerId);
    if (!workspaceId || !target || target.peerId !== peerId || target.remoteTabId !== session.tab.id
      || session.peerId || session.tab.remote_peer_id || session.tab.remote_tab_id) continue;
    const tab = projectAttachedRemoteTab(session.tab, { peer_id: peerId, peer_name: peer.name }, workspaceId);
    result.set(tab.id, { tab, workspaceId, ...placement, peerName, status: session.status });
  }
  return result;
}

type MentionPeerIdentity = Pick<PairedGeneral, "peerId" | "name" | "instanceName" | "installation">;
export type PeerMentionResult = PeerMentionTab | "Ambiguous peer" | "Ambiguous tab" | undefined;

function matchingPeers(peers: readonly MentionPeerIdentity[], ref: string): MentionPeerIdentity[] {
  const needle = ref.toLowerCase();
  const exact = peers.filter(peer => peer.peerId === ref || peer.installation?.installation_id === ref);
  if (exact.length) return exact;
  if (/^[a-f0-9-]{6,}$/i.test(ref)) {
    const prefixes = peers.filter(peer => peer.peerId.toLowerCase().startsWith(needle)
      || peer.installation?.installation_id.toLowerCase().startsWith(needle));
    if (prefixes.length) return prefixes;
  }
  return peers.filter(peer => {
    const qualifier = /^(.*) \[([^\]]*)\]$/.exec(peer.instanceName ?? "");
    const devName = qualifier?.[2].startsWith("dev-") ? qualifier[2].slice(4) : undefined;
    return [peer.name, peer.installation?.name, peer.instanceName, qualifier?.[1], devName]
      .some(name => name?.toLowerCase() === needle);
  });
}

export function resolvePeerMentionTab(
  inventory: MentionPeerInventory,
  targetId: string,
  cache?: Map<string, Map<string, PeerMentionTab>>,
  attached: Readonly<Record<string, Tab>> = {},
): PeerMentionResult {
  // A mention may name a machine with spaces or punctuation. Only its resolved ID is a transport route.
  const slash = targetId.lastIndexOf("/");
  if (!targetId.startsWith("mesh:") || slash <= 5) return undefined;
  const peerRef = targetId.slice(5, slash);
  const tabRef = targetId.slice(slash + 1);
  if (!tabRef || /[?&#\s]/.test(tabRef)) return undefined;
  const candidates = new Map<string, MentionPeerIdentity>(inventory.generals.map(peer => [peer.peerId, peer]));
  const attachedTabs = Object.values(attached);
  for (const tab of attachedTabs) {
    const peerId = tab.remote_peer_id;
    if (!peerId || !tab.remote_tab_id || candidates.has(peerId) || tab.id !== `mesh:${peerId}/${tab.remote_tab_id}`) continue;
    const instanceName = tab.remote_peer_name ?? undefined;
    candidates.set(peerId, { peerId, instanceName, name: cleanMachineName(instanceName, peerId) ?? instanceName ?? "" });
  }
  const peers = matchingPeers([...candidates.values()], peerRef);
  if (peers.length > 1) return "Ambiguous peer";
  if (!peers.length) return undefined;
  const { peerId, name } = peers[0];
  let tabs = cache?.get(peerId);
  if (!tabs) {
    tabs = peerMentionTabs(inventory, peerId);
    cache?.set(peerId, tabs);
  }
  const id = `mesh:${peerId}/${tabRef}`;
  const exact = tabs.get(id);
  if (exact) return exact;
  const matches = new Map([...tabs].filter(([key]) => key.startsWith(id)));
  for (const tab of attachedTabs) {
    if (tab.remote_peer_id !== peerId || !tab.remote_tab_id?.startsWith(tabRef)
      || !tab.workspace_id || tab.id !== `mesh:${peerId}/${tab.remote_tab_id}`) continue;
    const result = { tab, workspaceId: tab.workspace_id, peerName: name };
    if (tab.id === id) return result;
    if (!matches.has(tab.id)) matches.set(tab.id, result);
  }
  return matches.size > 1 ? "Ambiguous tab" : matches.values().next().value;
}
