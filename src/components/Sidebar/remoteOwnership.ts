import type { Tab } from "../../lib/types";

export interface RemoteTeamIdentity {
  id: string;
  title: string;
  peerName: string;
}

function availabilityNote(tab: Tab): string | null {
  if (tab.remote_availability === "reconnecting") return "Reconnecting";
  if (tab.remote_availability === "unavailable") return "Unavailable";
  return null;
}

export function managedByLabel(tab: Tab): string | null {
  const controller = tab.ownership?.controller_title?.trim();
  return controller ? `managed by ${controller}` : null;
}

export function ownershipSubtitle(tab: Tab): string | null {
  const base = managedByLabel(tab);
  if (!base) return null;
  const note = availabilityNote(tab);
  return note ? `${base} · ${note}` : base;
}

export function remoteTeamIdentity(tab: Tab): RemoteTeamIdentity | null {
  if (tab.remote_peer_id) return null;
  const owner = tab.ownership;
  if (!owner?.team || !owner.peer_id) return null;
  return {
    id: `mesh:${owner.peer_id}/${owner.team.id}`,
    title: owner.team.title?.trim() || owner.team.id,
    peerName: owner.peer_name,
  };
}
