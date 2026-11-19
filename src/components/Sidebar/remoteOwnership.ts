import { use, useContext, useSyncExternalStore } from "react";
import { MuxStateStoreContext, PairedWorkspaceInventoryContext } from "../../store/multiplexer/context";
import { cleanMachineName } from "../../lib/machineIdentity";
import type { PairedGeneral } from "../../store/multiplexer/pairedWorkspaceProjection";
import type { Tab } from "../../lib/types";
import { roleDisplayLabel, roleKindLabel, hasTeamAuthority } from "../../lib/roleKind";

export interface RemoteTeamIdentity {
  id: string;
  title: string;
  peerName?: string;
}

export function sharesObservedMachine(teamHostId?: string | null, workerHostId?: string | null): boolean {
  return Boolean(teamHostId && workerHostId && teamHostId === workerHostId);
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
  const peer = cleanMachineName(tab.remote_peer_name, tab.remote_peer_id) ?? "Machine";
  return { role: namedManagerRole(tab), peer };
}

export function remoteTeamName(teamId: string | null | undefined, ...names: Array<string | null | undefined>): string | undefined {
  return names.map(name => name?.trim()).find(name => name && name !== teamId
    && !/^team_[\da-f]+$/i.test(name) && cleanMachineName(name));
}

export function remoteTeamIdentity(tab: Tab, peers: readonly PairedGeneral[] = []): RemoteTeamIdentity | null {
  if (tab.remote_peer_id) return null;
  const owner = tab.ownership;
  if (!owner?.team || !owner.peer_id) return null;
  const peer = peers.find(candidate => candidate.peerId === owner.peer_id);
  return {
    id: `mesh:${owner.peer_id}/${owner.team.id}`,
    title: remoteTeamName(owner.team.id, peer?.teamNames?.get(owner.team.id), owner.team.title) ?? "Team",
    peerName: cleanMachineName(owner.peer_name, owner.peer_id),
  };
}

const noSubscription = () => () => {};

export function useRemoteTabTeamName(tab: Tab, peerId?: string | null): string | undefined {
  const teamId = tab.remote_team_id ?? tab.team_id ?? tab.ownership?.team?.id;
  const store = useContext(MuxStateStoreContext);
  const readLocalTeamName = () => peerId && tab.team_id
    ? store?.getState().teams?.find(team => team.id === tab.team_id)?.name
    : undefined;
  const localTeamName = useSyncExternalStore(
    peerId && tab.team_id && store ? store.subscribe : noSubscription,
    readLocalTeamName, readLocalTeamName,
  );
  if (!peerId || !teamId) return undefined;
  const { generals } = use(PairedWorkspaceInventoryContext);
  const peerTeamName = generals.find(peer => peer.peerId === peerId)?.teamNames?.get(teamId);
  const ownershipName = tab.ownership?.team?.id === teamId ? tab.ownership.team.title : undefined;
  return remoteTeamName(teamId, peerTeamName, localTeamName, ownershipName) ?? "Team";
}
