import { daemonTransportForPeer } from "./daemonBootstrap";
import type { MachineIdentity } from "./machineIdentity";

export interface MachineIdentityView {
  name: string;
  identity: MachineIdentity;
  chosen: boolean;
}

const route = (peerId?: string) => peerId ? "/api/mesh/identity" : "/api/account/mesh/identity";
export async function readMachineIdentity(peerId?: string): Promise<MachineIdentityView> {
  const result = await daemonTransportForPeer(peerId).json<MachineIdentityView>("GET", route(peerId));
  if (typeof result?.name !== "string" || typeof result.chosen !== "boolean" || !result.identity) {
    throw new Error("Machine identity is unavailable.");
  }
  return result;
}

export async function saveMachineIdentity(identity: MachineIdentity, peerId?: string): Promise<MachineIdentity> {
  const result = await daemonTransportForPeer(peerId).json<{ state: string; identity?: MachineIdentity }>("POST", route(peerId), identity);
  if (result.state !== "saved" || !result.identity) throw new Error("Owner approval is required.");
  return result.identity;
}
