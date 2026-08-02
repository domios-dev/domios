import type { Tab } from "../../lib/types";

export interface RemoteTabPeer {
  id: string;
  name: string;
  authenticated: boolean;
  reachable: boolean;
  reconnecting?: boolean;
  unavailable?: boolean;
  tabs: Tab[];
}

export function remoteTabId(peerId: string, tabId: string): string {
  return `mesh:${peerId}/${tabId}`;
}

export function mergeOrdinaryTabProjection(
  localTabs: Record<string, Tab>,
  peers: RemoteTabPeer[],
): Record<string, Tab> {
  const projected = { ...localTabs };
  for (const peer of peers) {
    if (!peer.authenticated || (!peer.reachable && !peer.reconnecting && !peer.unavailable)) continue;
    for (const tab of peer.tabs) {
      const id = remoteTabId(peer.id, tab.id);
      projected[id] = {
        ...tab,
        id,
        remote_peer_id: peer.id,
        remote_peer_name: peer.name,
        remote_tab_id: tab.id,
        remote_availability: peer.reachable
          ? "available"
          : peer.reconnecting
            ? "reconnecting"
            : "unavailable",
      };
    }
  }
  return projected;
}
