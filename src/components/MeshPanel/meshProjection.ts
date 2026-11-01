import type { Service } from "./types";
import type { MeshData, PeerStatus } from "./useMeshData";

/** A failed load with no live or cached data: any graph would be placeholders, never "0 peers". */
export function meshDataUnavailable(data: Pick<MeshData, "error" | "loaded">): boolean {
  return data.error !== null && !data.loaded;
}

export type MeshHeaderState =
  | { kind: "off" }
  | { kind: "unavailable" }
  | { kind: "loading" }
  | { kind: "peers"; count: number };

/** The header renders from live or cached data only; the defaults are never shown as "0 peers". */
/** A disabled mesh refuses every route by design, so it is "off", never a failure to retry. */
export function meshHeaderState(data: Pick<MeshData, "error" | "loaded" | "peers">, onInventory: boolean, meshEnabled = true): MeshHeaderState {
  if (!meshEnabled) return { kind: "off" };
  if (meshDataUnavailable(data)) return { kind: "unavailable" };
  if (!data.loaded) return { kind: "loading" };
  const count = pairedPeers(data.peers).length;
  if (data.error && !onInventory && count === 0) return { kind: "unavailable" };
  return { kind: "peers", count };
}

/** Key `serviceByPeer` uses for this machine's own (non-remote) services. */
export const SELF_PEER_KEY = "__self__";

/**
 * Services the panel actually renders. Cron jobs are virtual services with
 * their own dedicated surface (CronsTable) — excluded here so the header
 * count and every peer card count the exact same set (codeterm#440).
 */
export function visibleServices(services: Service[]): Service[] {
  return services.filter((s) => s.kind !== "cron");
}

/** Group visible services by owning peer id, so the header total and each
 * peer card are computed from the SAME grouping instead of two separate
 * derivations that can silently disagree. */
export function groupServicesByPeer(services: Service[]): Map<string, Service[]> {
  const grouped = new Map<string, Service[]>();
  grouped.set(SELF_PEER_KEY, []);
  for (const s of visibleServices(services)) {
    const key = s.source_peer_id || s.source_peer || SELF_PEER_KEY;
    const list = grouped.get(key);
    if (list) list.push(s);
    else grouped.set(key, [s]);
  }
  return grouped;
}

/**
 * Peers actually paired — excludes the synthetic vouched-but-not-yet-paired
 * rows `useMeshData` renders in the same list so the UI can show an
 * "auto-pairs when online" affordance. A discovered-not-paired peer must
 * never count toward, or render as, "paired" (codeterm#440).
 */
export function pairedPeers(peers: PeerStatus[]): PeerStatus[] {
  return peers.filter((p) => !p.pending);
}

export const WEB_NODE_ID = "__web__";

export function webGatewayGraph(peers: PeerStatus[], localOnline = true) {
  return {
    nodes: [WEB_NODE_ID, SELF_PEER_KEY, ...peers.map((peer) => peer.instance_id)],
    edges: [
      { machineId: SELF_PEER_KEY, online: localOnline, label: "via this machine's gateway" },
      ...pairedPeers(peers).map((peer) => ({ machineId: peer.instance_id, online: peer.online, label: "via this machine's gateway" })),
    ],
  };
}
