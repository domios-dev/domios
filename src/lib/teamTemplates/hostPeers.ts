import { localJson } from "../webApi";

type MeshPeersResponse = import("../../../gen/MeshPeersResponse").MeshPeersResponse;
type PairedPeer = import("../../../gen/PairedPeer").PairedPeer;

export interface HostPeer { id: string; name: string }

/** Peers that can host a Team or a role: paired, online, and with Full access. */
export function hostPeersOf(paired: readonly Pick<PairedPeer, "instance_id" | "instance_name" | "online" | "access_level">[]): HostPeer[] {
  return paired
    .filter((peer) => peer.online && peer.access_level === "full")
    .map((peer) => ({ id: peer.instance_id, name: peer.instance_name }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

export async function listHostPeers(): Promise<HostPeer[]> {
  const peers = await localJson<Pick<MeshPeersResponse, "paired">>("GET", "/api/mesh/peers");
  return hostPeersOf(peers.paired ?? []);
}
