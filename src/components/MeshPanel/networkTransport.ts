import type { TailscaleState } from "./tailscaleSetup";

import type { NetworkIdentity, NetworkTransportStatus } from "../../lib/types";
export type { NetworkIdentity, NetworkTransportStatus } from "../../lib/types";

export function transportState(status: NetworkTransportStatus): TailscaleState {
  switch (status.state) {
    case "connected": return { kind: "connected", tailnet: status.network_name };
    case "starting": return { kind: "checking" };
    default: return { kind: status.state };
  }
}

export function differentNetwork(local: NetworkIdentity | null | undefined, remote: NetworkIdentity | null | undefined): boolean {
  return !!local && !!remote && local.kind === remote.kind && local.network_id !== remote.network_id;
}
