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

function findMentionPeer(inventory: MentionPeerInventory, peerRef: string): PairedGeneral | undefined {
  const exact = inventory.generals.find(row => row.peerId === peerRef);
  if (exact) return exact;
  const lowered = peerRef.toLowerCase();
  const byName = inventory.generals.filter(row => {
    const display = cleanMachineName(row.name, row.peerId) ?? row.name;
    return row.name.toLowerCase() === lowered || display.toLowerCase() === lowered;
  });
  return byName.length === 1 ? byName[0] : undefined;
}

/** Resolves a `mesh:<peer>/<tab>` mention by peer id or unique peer name, and exact or unique-prefix tab id. */
export function resolvePeerMentionTab(
  inventory: MentionPeerInventory,
  targetId: string,
  cache?: Map<string, Map<string, PeerMentionTab>>,
): PeerMentionTab | undefined {
  const route = parseCompositeTabId(targetId);
  if (!route) return undefined;
  const peer = findMentionPeer(inventory, route.peerId);
  if (!peer) return undefined;
  let tabs = cache?.get(peer.peerId);
  if (!tabs) {
    tabs = peerMentionTabs(inventory, peer.peerId);
    cache?.set(peer.peerId, tabs);
  }
  const exact = tabs.get(`mesh:${peer.peerId}/${route.remoteTabId}`);
  if (exact) return exact;
  const matches = [...tabs.values()].filter(entry => entry.tab.remote_tab_id?.startsWith(route.remoteTabId));
  return matches.length === 1 ? matches[0] : undefined;
}
