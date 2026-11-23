import { useSyncExternalStore } from "react";
import { cleanMachineName, type MachineIdentity, type MeshIdentityProjection } from "./machineIdentity";
import { accountMachineNameProjection } from "./registryMachineNames";
import type { AccountMeshSnapshot } from "../components/MeshPanel/accountMeshClient";
import type { MeshPeersResponse } from "./types";

const subscribers = new Set<() => void>();
type Snapshot = MeshIdentityProjection & Pick<MeshPeersResponse, "installations"> & { peers?: MeshPeersResponse["paired"] };
const empty: Snapshot = {};
let snapshot = empty;
let accountNames: { local?: string; peers: ReadonlyMap<string, string> } = { peers: new Map() };

export function rememberAccountMachineNames(account: AccountMeshSnapshot | null) {
  const { local, peers } = accountMachineNameProjection(account);
  if (local === accountNames.local && peers.size === accountNames.peers.size &&
    [...peers].every(([id, name]) => accountNames.peers.get(id) === name)) return;
  accountNames = { local, peers };
  subscribers.forEach(listener => listener());
}

export function accountPeerMachineName(peerId: string): string | undefined {
  return accountNames.peers.get(peerId);
}

export function useAccountMachineNames() {
  return useSyncExternalStore(subscribe, () => accountNames, () => accountNames);
}

function subscribe(listener: () => void) {
  subscribers.add(listener);
  return () => { subscribers.delete(listener); };
}

export function rememberMachineIdentities(peers: MeshPeersResponse & MeshIdentityProjection) {
  snapshot = {
    local_instance_name: cleanMachineName(peers.local_instance_name),
    local_machine_identity: peers.local_machine_identity,
    machine_identities: peers.machine_identities,
    peers: peers.paired,
    installations: peers.installations,
  };
  subscribers.forEach(listener => listener());
}

const read = () => snapshot;
const ignoreSubscription = () => () => undefined;

export function useMeshMachineIdentities(): MeshIdentityProjection {
  return useSyncExternalStore(subscribe, read, read);
}

export function useMeshMachineChoices(): Snapshot {
  return useSyncExternalStore(subscribe, read, read);
}

export function useMeshPeerName(peerId?: string | null): string | undefined {
  const readName = () => {
    if (!peerId) return undefined;
    const peer = snapshot.peers?.find(peer => peer.instance_id === peerId);
    const installation = snapshot.installations?.find(candidate =>
      candidate.instance_id === peerId || candidate.installation_id === peerId);
    return accountPeerMachineName(peerId) ?? cleanMachineName(peer?.instance_name, peerId) ?? cleanMachineName(installation?.name, peerId);
  };
  return useSyncExternalStore(peerId ? subscribe : ignoreSubscription, readName, readName);
}

export function useMeshMachineIdentity(peerId?: string | null, enabled = true, peerName?: string): MachineIdentity | undefined {
  const readIdentity = () => {
    if (!enabled) return undefined;
    if (peerId) return snapshot.machine_identities?.[peerId];
    if (peerName) {
      const matches = snapshot.peers?.filter(peer => cleanMachineName(peer.instance_name) === cleanMachineName(peerName));
      return matches?.length === 1 ? snapshot.machine_identities?.[matches[0].instance_id] : undefined;
    }
    return snapshot.local_machine_identity;
  };
  return useSyncExternalStore(enabled ? subscribe : ignoreSubscription, readIdentity, readIdentity);
}
