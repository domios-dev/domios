import { useMemo } from "react";
import { accountCanvas } from "./accountCanvas";
import type { MachineRowView } from "./accountMachines";
import type { MeshTarget } from "./accountMeshClient";
import type { CanvasAccount } from "./MeshCanvas";
import type { PeerStatus } from "./useMeshData";
import { useMeshAccount } from "./useMeshAccount";
import { useMyMachines } from "./useMyMachines";

/** My machines projected onto the Mesh canvas, live with the account and the paired peers. */
export function useCanvasAccount(peers: readonly PeerStatus[], onChanged: () => void): CanvasAccount {
  const account = useMeshAccount();
  const machines = useMyMachines({ account: account.presence, onSignIn: account.signIn, onChanged });
  const { view, snapshot, loadFailed, listFailure, failures, busy, signingIn, connect, decide, disconnect, openPlans, dismissFailure, signIn, load } = machines;
  const canvas = useMemo(
    () => accountCanvas({ view, peers, loadFailed, listFailed: listFailure !== null, failures }),
    [view, peers, loadFailed, listFailure, failures],
  );
  // Ages are read against the moment the facts behind them last changed.
  const now = useMemo(() => Date.now(), [snapshot, peers]);
  const actions = useMemo(() => ({
    onDecide: (row: MachineRowView, notificationId: string, approve: boolean) => { void decide(row, notificationId, approve); },
    onConnect: (row: MachineRowView, target: MeshTarget) => { void connect(row, target); },
    onDisconnect: (row: MachineRowView, instanceId: string) => { void disconnect(row, instanceId); },
    onPlans: openPlans,
    onDismiss: dismissFailure,
  }), [connect, decide, disconnect, openPlans, dismissFailure]);
  return useMemo(() => ({
    canvas, busy, signingIn, now, actions, reauthRequired: view.kind === "account" && view.reauthRequired,
    onSignIn: () => { void signIn(); },
    onRetry: () => { void load(); },
  }), [canvas, busy, signingIn, now, actions, view, signIn, load]);
}
