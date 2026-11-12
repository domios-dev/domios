import type { Tab } from "../../lib/types";
import { roleDisplayLabel, roleKindLabel, hasTeamAuthority } from "../../lib/roleKind";

export interface RemoteTeamIdentity {
  id: string;
  title: string;
  peerName: string;
}

export function managedByLabel(tab: Tab): string | null {
  const controller = tab.ownership?.controller_title?.trim();
  return controller ? `managed by ${controller}` : null;
}

export function namedManagerRole(tab: Tab): string | null {
  const profile = tab.role_profile?.trim();
  // A profile is a key: built-in kinds read as their host name, others in sentence case.
  if (profile) return roleDisplayLabel(profile);
  return hasTeamAuthority(tab) ? roleKindLabel(tab) : null;
}

export function remoteIdentity(tab: Tab): { role: string | null; peer: string } | null {
  if (!tab.remote_peer_id) return null;
  const peer = tab.remote_peer_name?.trim() || tab.remote_peer_id;
  return { role: namedManagerRole(tab), peer: `@${peer}` };
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

export function meshBadgeLabel(tab: Tab): string | null {
  const identity = remoteIdentity(tab);
  return identity ? `on ${identity.peer}` : null;
}
