import type { Tab } from "./types";
import { invoke } from "./bridge";

export interface RemoteTabRoute {
  peerId: string;
  tabId: string;
}

export interface RemoteTabActionRequest extends Record<string, unknown> {
  ordinaryTabId: string;
  peerId: string;
  method: string;
  suffix: string;
  body: unknown;
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

export async function relayRemoteTabAction<T>(
  route: RemoteTabRoute,
  method: string,
  suffix: string,
  body: unknown = null,
): Promise<T> {
  return invoke<T>("mesh_remote_tab_action", remoteTabActionRequest(route, method, suffix, body));
}

export function remoteTabActionRequest(
  route: RemoteTabRoute,
  method: string,
  suffix: string,
  body: unknown = null,
): RemoteTabActionRequest {
  return {
    ordinaryTabId: `mesh:${route.peerId}/${route.tabId}`,
    peerId: route.peerId,
    method,
    suffix,
    body,
  };
}

export function remoteTabStreamPath(route: RemoteTabRoute): string {
  const ordinary = `mesh:${route.peerId}/${route.tabId}`;
  return `/api/mesh/relay-stream?peer=${encodeURIComponent(route.peerId)}&path=${encodeURIComponent("/api/output/stream")}&tab=${encodeURIComponent(ordinary)}`;
}
