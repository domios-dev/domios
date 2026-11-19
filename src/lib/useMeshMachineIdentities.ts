import { useSyncExternalStore } from "react";
import type { MachineIdentity, MeshIdentityProjection } from "./machineIdentity";
import type { MeshPeersResponse } from "./types";

const subscribers = new Set<() => void>();
type Snapshot = MeshIdentityProjection & { peers?: MeshPeersResponse["paired"] };
const empty: Snapshot = {};
let snapshot = empty;

function subscribe(listener: () => void) {
  subscribers.add(listener);
  return () => { subscribers.delete(listener); };
}

export function rememberMachineIdentities(peers: MeshPeersResponse & MeshIdentityProjection) {
  snapshot = {
    local_instance_name: peers.local_instance_name?.trim() || undefined,
    local_machine_identity: peers.local_machine_identity,
    machine_identities: peers.machine_identities,
    peers: peers.paired,
  };
  subscribers.forEach(listener => listener());
}

const read = () => snapshot;
const ignoreSubscription = () => () => undefined;

export function useMeshMachineIdentities(): MeshIdentityProjection {
  return useSyncExternalStore(subscribe, read, read);
}

export function useMeshMachineIdentity(peerId?: string | null, enabled = true, peerName?: string): MachineIdentity | undefined {
  const readIdentity = () => {
    if (!enabled) return undefined;
    if (peerId) return snapshot.machine_identities?.[peerId];
    if (peerName) {
      const matches = snapshot.peers?.filter(peer => peer.instance_name === peerName);
      return matches?.length === 1 ? snapshot.machine_identities?.[matches[0].instance_id] : undefined;
    }
    return snapshot.local_machine_identity;
  };
  return useSyncExternalStore(enabled ? subscribe : ignoreSubscription, readIdentity, readIdentity);
}
