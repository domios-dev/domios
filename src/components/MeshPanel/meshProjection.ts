import type { Service } from "./types";
import type { PeerStatus } from "./useMeshData";

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
