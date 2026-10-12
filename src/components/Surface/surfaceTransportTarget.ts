import type { Tab } from "../../lib/types";
import { tabOutputRoute } from "../../lib/remoteTabRoute";

export interface SurfaceTransportTarget {
  tabId: string;
  peerId?: string;
}

export function surfaceTransportTarget(
  tab: Pick<Tab, "id" | "remote_peer_id" | "remote_tab_id">,
  selectedPeerId: string | null,
): SurfaceTransportTarget {
  const route = tabOutputRoute(tab.id, selectedPeerId);
  if (tab.remote_peer_id || tab.remote_tab_id) {
    if (!route || route.peerId !== tab.remote_peer_id || route.tabId !== tab.remote_tab_id) {
      throw new Error(`Surface tab ${tab.id} has inconsistent daemon ownership`);
    }
  }
  return route ? { tabId: route.tabId, peerId: route.peerId } : { tabId: tab.id };
}
