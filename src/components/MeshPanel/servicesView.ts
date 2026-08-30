import type { Service, InventoryProvenance } from "./types";

/** Envelope returned by the `mesh_peer_services_list` Tauri command. */
export interface PeerServicesView {
  services: Service[];
  stale: boolean;
  fetched_at: number | null;
  peer_online: boolean;
  error: string | null;
  freshness: "fresh" | "stale" | "offline";
  partial_error: string | null;
}

export interface MeshInventoryView {
  contract_version: number;
  machine_id: string;
  machine_name: string;
  resources: Service[];
  provenance: InventoryProvenance;
}

export interface PeerFetchResult {
  peerId: string;
  peerName: string;
  /** null when the invoke itself failed (peer unpaired mid-poll, etc.). */
  view: PeerServicesView | null;
}

/**
 * Merge local services with per-peer envelopes into the table's row list.
 * Stale (cached) peer services stay visible and carry flags for rendering —
 * they must never silently drop out of the list (#119/#127).
 */
export function mergeServicesView(
  local: Service[],
  remote: PeerFetchResult[],
): Service[] {
  const out = [...local];
  for (const r of remote) {
    if (!r.view) continue;
    for (const s of r.view.services) {
      out.push({
        ...s,
        source_peer_id: r.peerId,
        source_peer: r.peerName,
        stale: r.view.stale,
        peer_online: r.view.peer_online,
        machine_id: s.machine_id ?? r.peerId,
        machine_name: s.machine_name ?? r.peerName,
        provenance: s.provenance ?? {
          freshness: r.view.freshness,
          offline: !r.view.peer_online,
          fetched_at: r.view.fetched_at ?? undefined,
          partial_error: r.view.partial_error ?? undefined,
        },
      });
    }
  }
  return out;
}

/** Non-empty fetch errors, prefixed with the peer name, for diagnostics. */
export function collectPeerErrors(remote: PeerFetchResult[]): string[] {
  const errors: string[] = [];
  for (const r of remote) {
    if (r.view?.error) errors.push(`${r.peerName}: ${r.view.error}`);
  }
  return errors;
}
