import type { BrowsedWorkspace, PairedGeneral } from "../../store/multiplexer/pairedWorkspaceProjection";
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
  for (const { session, workspaceId, ...placement } of records) {
    const target = parseCompositeTabId(session.viewerId);
    if (!workspaceId || !target || target.peerId !== peerId || target.remoteTabId !== session.tab.id
      || session.peerId || session.tab.remote_peer_id || session.tab.remote_tab_id) continue;
    const tab = projectAttachedRemoteTab(session.tab, { peer_id: peerId, peer_name: peer.name }, workspaceId);
    result.set(tab.id, { tab, workspaceId, ...placement, status: session.status });
  }
  return result;
}
