import type { Tab } from "../../lib/types";

export interface RemoteOwnership {
  peer: string;
  controller: string | null;
  managedByLabel: string;
  availabilityNote: string | null;
}

export function remoteOwnership(tab: Tab): RemoteOwnership | null {
  const peer = tab.remote_peer_name?.trim();
  if (!tab.remote_peer_id || !peer) return null;
  const controller = tab.remote_controller_title?.trim() || null;
  const managedByLabel = controller ? `managed by ${controller}` : `managed by @${peer}`;
  const availabilityNote =
    tab.remote_availability === "reconnecting"
      ? "Reconnecting"
      : tab.remote_availability === "unavailable"
        ? "Unavailable"
        : null;
  return { peer, controller, managedByLabel, availabilityNote };
}

export function remoteOwnershipSubtitle(ownership: RemoteOwnership): string {
  return ownership.availabilityNote
    ? `${ownership.managedByLabel} · ${ownership.availabilityNote}`
    : ownership.managedByLabel;
}
