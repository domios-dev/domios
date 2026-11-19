import { cleanMachineName, type MachineIdentity, type MeshIdentityProjection } from "./machineIdentity";
import { RELAY_PROTOCOL_VERSION, type MeshPeersResponse, type PairedPeer } from "./types";

export const LOCAL_MACHINE_ID = "__local_machine__";

export interface MachinePlacementPeer {
  id: string;
  name: string;
  online: boolean;
  accessLevel: PairedPeer["access_level"];
  relayProtocolVersion: number;
  identity?: MachineIdentity;
}

export interface MachinePlacementChoice {
  id: string;
  name: string;
  local: boolean;
  online: boolean;
  status: string;
  unavailableReason: string | null;
  identity?: MachineIdentity;
}

function peerChoice(peer: MachinePlacementPeer): MachinePlacementChoice {
  if (!peer.online) {
    return { ...peer, local: false, status: "offline", unavailableReason: "This machine is offline." };
  }
  if (peer.relayProtocolVersion !== RELAY_PROTOCOL_VERSION) {
    return { ...peer, local: false, status: "needs update", unavailableReason: "This machine needs a compatible Domios update." };
  }
  if (peer.accessLevel !== "full") {
    return { ...peer, local: false, status: "restricted", unavailableReason: "Full access is required to start on this machine." };
  }
  return { ...peer, local: false, status: "online", unavailableReason: null };
}

export function machinePlacementChoices(
  response: MeshPeersResponse & MeshIdentityProjection,
): MachinePlacementChoice[] {
  const peers = (response.paired ?? []).map((peer) => ({
    id: peer.instance_id,
    name: peer.instance_name,
    online: peer.online,
    accessLevel: peer.access_level,
    relayProtocolVersion: peer.relay_protocol_version,
    identity: response.machine_identities?.[peer.instance_id],
  }));
  return machinePlacementChoicesFromPeers(
    response.local_instance_name,
    response.local_machine_identity,
    peers,
  );
}

export function machinePlacementChoicesFromPeers(
  localName: string | undefined,
  localIdentity: MachineIdentity | undefined,
  peers: readonly MachinePlacementPeer[],
): MachinePlacementChoice[] {
  const local: MachinePlacementChoice = {
    id: LOCAL_MACHINE_ID,
    name: cleanMachineName(localName) ?? "This machine",
    local: true,
    online: true,
    status: "online",
    unavailableReason: null,
    identity: localIdentity,
  };
  const remote = peers
    .filter((peer) => peer.id !== LOCAL_MACHINE_ID)
    .map((peer) => peerChoice({ ...peer, name: cleanMachineName(peer.name, peer.id) ?? "Paired machine" }))
    .sort((left, right) => left.name.localeCompare(right.name) || left.id.localeCompare(right.id));
  return [local, ...remote];
}
