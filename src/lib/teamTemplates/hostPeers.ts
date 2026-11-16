import { invoke } from "../bridge";
import { failureMessage } from "../../components/MeshPanel/accountMachines";
import type { AccountMeshFailure } from "../../components/MeshPanel/accountMeshClient";

type MeshPeersResponse = import("../../../gen/MeshPeersResponse").MeshPeersResponse;
type PairedPeer = import("../../../gen/PairedPeer").PairedPeer;

export interface HostPeer { id: string; name: string }

/** Peers that can host a Team, or the plain reason none is listed. */
export interface HostPeerListing { peers: HostPeer[]; reason: string | null }

/** A workspace on the peer the Team can start in. */
export interface PeerWorkspace { id: string; name: string }

export const NO_HOST_PEER = "No paired machine with Full access is online.";

/** Peers that can host a Team: paired, online, and with Full access. */
export function hostPeersOf(paired: readonly Pick<PairedPeer, "instance_id" | "instance_name" | "online" | "access_level">[]): HostPeer[] {
  return paired
    .filter((peer) => peer.online && peer.access_level === "full")
    .map((peer) => ({ id: peer.instance_id, name: peer.instance_name }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** An empty list says why: this machine's own account-mesh refusal comes first. */
export function emptyHostReason(refusal: AccountMeshFailure | null | undefined): string {
  return refusal ? `This machine can't reach your account mesh: ${failureMessage(refusal)}` : NO_HOST_PEER;
}

export function hostListingFailure(error: unknown): string {
  return `This machine can't list its paired machines: ${error instanceof Error ? error.message : String(error)}`;
}

export async function listHostPeers(): Promise<HostPeerListing> {
  const response = await invoke<Pick<MeshPeersResponse, "paired">>("mesh_list_peers");
  const peers = hostPeersOf(response.paired ?? []);
  if (peers.length > 0) return { peers, reason: null };
  const mesh = await invoke<{ last_refusal?: AccountMeshFailure | null }>("account_mesh_state").catch(() => null);
  return { peers, reason: emptyHostReason(mesh?.last_refusal) };
}

/** The peer's own workspaces a Team can join; its protected home workspace is not one of them. */
export function peerWorkspacesOf(raw: unknown): PeerWorkspace[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((item): item is { id: string; name?: string; protected?: boolean } =>
      typeof item?.id === "string" && item.protected !== true)
    .map((item) => ({ id: item.id, name: item.name || item.id }));
}

export async function listPeerWorkspaces(peerId: string): Promise<PeerWorkspace[]> {
  return peerWorkspacesOf(await invoke<unknown>("mesh_peer_workspaces_list", { peerId }));
}

/** Creates a provisioned workspace on the peer without moving its owner's focus; returns its id. */
export async function createPeerWorkspace(peerId: string, name: string): Promise<string> {
  const created = await invoke<{ id?: string; workspace_id?: string }>("mesh_peer_workspace_create", { peerId, name });
  const id = created?.id ?? created?.workspace_id;
  if (!id) throw new Error("the peer created no workspace");
  return id;
}
