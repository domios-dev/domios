import type { AccountCanvas } from "./accountCanvas";
import type { MachineRowView } from "./accountMachines";
import type { TailscaleState } from "./tailscaleSetup";
import type { PeerStatus } from "./useMeshData";

export type MeshOverviewState =
  | { kind: "signed_out" }
  | { kind: "only_this_machine" }
  | { kind: "tailscale_off" }
  | { kind: "unreachable"; machine: MachineRowView }
  | { kind: "inventory" };

export function meshOverviewState(
  account: AccountCanvas,
  peers: readonly PeerStatus[],
  tailscale?: TailscaleState,
  moreMachines = false,
): MeshOverviewState {
  if (account.card?.kind === "sign_in") return { kind: "signed_out" };
  if (moreMachines) return { kind: "inventory" };
  const off = tailscale && !["connected", "checking", "unknown"].includes(tailscale.kind);
  if (account.card?.kind === "empty" && !account.card.listFailed) {
    return { kind: off ? "tailscale_off" : "only_this_machine" };
  }
  if (!account.card && tailscale?.kind === "connected") {
    const rows = [...account.nodes.map(({ row }) => row), ...[...account.peerFooters.values()].map(({ row }) => row)];
    const machine = rows[0];
    if (rows.length === 1 && peers.length <= 1 && machine
      && ["unreachable", "endpoint_missing", "endpoint_stale"].includes(machine.status) && !machine.online) {
      return { kind: "unreachable", machine };
    }
  }
  return { kind: "inventory" };
}
