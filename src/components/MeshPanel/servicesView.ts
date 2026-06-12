import type { Service } from "./types";

/** Envelope returned by the `mesh_peer_services_list` Tauri command. */
export interface PeerServicesView {
  services: Service[];
  stale: boolean;
  fetched_at: number | null;
  peer_online: boolean;
  error: string | null;
}

export interface PeerFetchResult {
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
        source_peer: r.peerName,
        stale: r.view.stale,
        peer_online: r.view.peer_online,
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
