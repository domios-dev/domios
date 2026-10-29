import type { TailscaleState } from "./tailscaleSetup";

export interface NetworkIdentity { kind: string; network_id: string }
export interface NetworkTransportStatus {
  display_name: string;
  state: "not_installed" | "not_running" | "permission_needed" | "signed_out" | "starting" | "connected" | "unknown";
  identity: NetworkIdentity | null;
  network_name: string | null;
  user?: string | null;
  addresses: string[];
  peer_listener: boolean;
  encrypted: boolean;
  setup_actions: Array<{ kind: string; url: string }>;
}

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
