import type { Tab } from "./types";
import { invoke } from "./bridge";
import { classifyTabTarget, parseCompositeTabId } from "./remoteTabTransport";

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

export function tabOutputRoute(tabId: string, relayPeerId: string | null): RemoteTabRoute | null {
  if (classifyTabTarget(tabId).kind === "invalid-remote") {
    throw new Error(`Invalid remote tab id "${tabId}"; local output fallback is forbidden`);
  }
  const identity = parseCompositeTabId(tabId);
  if (identity) return { peerId: identity.peerId, tabId: identity.remoteTabId };
  if (!relayPeerId) return null;
  const scopedId = `mesh:${relayPeerId}/${tabId}`;
  if (!parseCompositeTabId(scopedId)) throw new Error(`Invalid remote tab id "${scopedId}"`);
  return { peerId: relayPeerId, tabId };
}

export function remoteTabStreamPath(route: RemoteTabRoute, since?: string): string {
  const ordinary = `mesh:${route.peerId}/${route.tabId}`;
  const path = `/api/mesh/relay-stream?peer=${encodeURIComponent(route.peerId)}&path=${encodeURIComponent("/api/output/stream")}&tab=${encodeURIComponent(ordinary)}`;
  return since ? `${path}&since=${encodeURIComponent(since)}` : path;
}
