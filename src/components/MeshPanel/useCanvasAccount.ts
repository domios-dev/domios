import { useEffect, useMemo, useRef } from "react";
import { accountCanvas } from "./accountCanvas";
import type { MachineRowView } from "./accountMachines";
import type { MeshTarget } from "./accountMeshClient";
import type { CanvasAccount } from "./MeshCanvas";
import type { PeerStatus } from "./useMeshData";
import { useMeshAccount } from "./useMeshAccount";
import { useMyMachines } from "./useMyMachines";

/** My machines projected onto the Mesh canvas, live with the account and the paired peers. */
export function useCanvasAccount(peers: readonly PeerStatus[], onChanged: () => void, registryRefreshKey = 0): CanvasAccount {
  const account = useMeshAccount();
  const machines = useMyMachines({ account: account.presence, onSignIn: account.signIn, onChanged });
  const { view, snapshot, loadFailed, listFailure, failures, busy, signingIn, connect, decide, disconnect, openPlans, dismissFailure, signIn, load, rename, renameSelf, revoke, revokeSession, retryEnrollment } = machines;
  const lastRefreshKey = useRef(registryRefreshKey);
  useEffect(() => {
    if (lastRefreshKey.current === registryRefreshKey) return;
    lastRefreshKey.current = registryRefreshKey;
    void load();
  }, [registryRefreshKey, load]);
  const canvas = useMemo(
    () => accountCanvas({ view, peers, loadFailed, listFailed: listFailure !== null, failures }),
    [view, peers, loadFailed, listFailure, failures],
  );
  // Ages are read against the moment the facts behind them last changed.
  const now = useMemo(() => Date.now(), [snapshot, peers]);
  const actions = useMemo(() => ({
    onSignOut: revokeSession,
    onRemove: revoke,
    onDecide: (row: MachineRowView, notificationId: string, approve: boolean) => { void decide(row, notificationId, approve); },
    onRename: rename,
    onConnect: (row: MachineRowView, target: MeshTarget) => { void connect(row, target); },
    onDisconnect: (row: MachineRowView, instanceId: string) => { void disconnect(row, instanceId); },
    onPlans: openPlans,
    onDismiss: dismissFailure,
  }), [connect, decide, disconnect, openPlans, dismissFailure, rename, revoke, revokeSession]);
  const self = view.kind === "account" ? view.self : null;
  return useMemo(() => ({
    moreMachines: !!snapshot?.registry?.cursor,
    canvas, busy, signingIn, now, actions, selfName: self?.displayName ?? undefined, onRenameSelf: self && !self.readOnly ? renameSelf : undefined, removed: view.kind === "account" && view.removed, onRejoin: () => { void signIn(); }, reauthRequired: view.kind === "account" && view.reauthRequired,
    limitMachines: view.kind === "account" && view.machineLimit ? machines : undefined,
    onSignIn: () => { void signIn(); },
    onRetry: () => { void load(); },
    onRetryEnrollment: account.presence === "signed_in" && snapshot?.enrollment === "refused" && snapshot.retry_at == null
      && ["unavailable", "offline", "authority-unavailable", "rate-limited", "enrollment_budget"].includes(snapshot.last_refusal?.reason ?? "")
      ? () => { void retryEnrollment(); } : undefined,
  }), [canvas, busy, signingIn, now, actions, self, renameSelf, view, signIn, load, snapshot, machines, account.presence, retryEnrollment]);
}
