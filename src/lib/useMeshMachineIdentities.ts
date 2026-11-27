import { useSyncExternalStore } from "react";
import { cleanMachineName, type MachineIdentity, type MeshIdentityProjection } from "./machineIdentity";
import { accountMachineNameProjection } from "./registryMachineNames";
import type { AccountMeshSnapshot } from "../components/MeshPanel/accountMeshClient";
import type { MeshPeersResponse } from "./types";
import type { CloudCostSummary, CloudReceipt, EnrichedMeshPeersResponse } from "./cloudMachines/types";

const subscribers = new Set<() => void>();
type Snapshot = MeshIdentityProjection & Pick<MeshPeersResponse, "installations"> & {
  peers?: MeshPeersResponse["paired"];
  cloudByPeer?: ReadonlyMap<string, CloudCostSummary | CloudReceipt>;
};
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

export function rememberMachineIdentities(peers: (MeshPeersResponse | EnrichedMeshPeersResponse) & MeshIdentityProjection) {
  const enriched = peers as EnrichedMeshPeersResponse;
  const identities: Record<string, MachineIdentity> = { ...(peers.machine_identities ?? {}) };
  const cloudByPeer = new Map<string, CloudCostSummary | CloudReceipt>();
  for (const peer of enriched.paired ?? []) {
    if (peer.kind === "cloud" || peer.cloud) {
      identities[peer.instance_id] = { kind: "vm" };
      if (peer.cloud) cloudByPeer.set(peer.instance_id, peer.cloud);
    }
  }
  for (const machine of enriched.cloud_machines ?? []) {
    for (const key of [machine.peer_instance_id, machine.installation_id, machine.machine_id]) {
      if (key) {
        identities[key] = { kind: "vm" };
        cloudByPeer.set(key, machine);
      }
    }
  }
  snapshot = {
    local_instance_name: cleanMachineName(peers.local_instance_name),
    local_machine_identity: peers.local_machine_identity,
    machine_identities: identities,
    peers: peers.paired,
    installations: peers.installations,
    cloudByPeer,
  };
  subscribers.forEach(listener => listener());
}

export function useCloudMachineForPeer(peerId?: string | null): CloudCostSummary | CloudReceipt | undefined {
  const readCloud = () => (peerId ? snapshot.cloudByPeer?.get(peerId) : undefined);
  return useSyncExternalStore(peerId ? subscribe : ignoreSubscription, readCloud, readCloud);
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
