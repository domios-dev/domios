import type { Tab } from "./types";

export interface RemoteTabRoute {
  peerId: string;
  tabId: string;
}

export function requireRemoteTabRoute(tab: Tab): RemoteTabRoute {
  if (!tab.remote_peer_id || !tab.remote_tab_id) {
    throw new Error(`Tab ${tab.id} is not an authenticated remote projection`);
  }
  if (tab.remote_availability !== "available") {
    throw new Error(`Remote tab ${tab.id} is ${tab.remote_availability ?? "unavailable"}`);
  }
  return { peerId: tab.remote_peer_id, tabId: tab.remote_tab_id };
}
