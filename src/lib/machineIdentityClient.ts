import { daemonTransportForPeer } from "./daemonBootstrap";
import { machineKind, type MachineIdentity } from "./machineIdentity";

export interface MachineIdentityView {
  name: string;
  identity: MachineIdentity;
  chosen: boolean;
}

// Keep sending the neutral legacy color until 1.23.2 peers leave the fleet.
const LEGACY_WIRE_COLOR = "graphite";

const route = (peerId?: string) => peerId ? "/api/mesh/identity" : "/api/account/mesh/identity";
export async function readMachineIdentity(peerId?: string): Promise<MachineIdentityView> {
  const result = await daemonTransportForPeer(peerId).json<MachineIdentityView>("GET", route(peerId));
  if (typeof result?.name !== "string" || typeof result.chosen !== "boolean" || !result.identity) {
    throw new Error("Machine identity is unavailable.");
  }
  return { ...result, identity: { kind: machineKind(result.identity.kind) } };
}

export async function saveMachineIdentity(identity: MachineIdentity, peerId?: string): Promise<MachineIdentity> {
  const payload = { kind: identity.kind, color: LEGACY_WIRE_COLOR };
  const result = await daemonTransportForPeer(peerId).json<{ state: string; identity?: MachineIdentity }>("POST", route(peerId), payload);
  if (result.state !== "saved" || !result.identity) throw new Error("Owner approval is required.");
  return { kind: machineKind(result.identity.kind) };
}
