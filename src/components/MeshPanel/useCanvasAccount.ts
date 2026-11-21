import { useEffect, useMemo, useRef } from "react";
import { accountCanvas } from "./accountCanvas";
import type { CanvasAccount } from "./MeshCanvas";
import type { PeerStatus } from "./useMeshData";
import type { MeshAccount } from "./useMeshAccount";
import { useMyMachines } from "./useMyMachines";

/** My machines projected onto the Mesh canvas, live with the account and the paired peers. */
export function useCanvasAccount(peers: readonly PeerStatus[], onChanged: () => void, registryRefreshKey: number, account: MeshAccount, enabled = true): CanvasAccount {
  const machines = useMyMachines({ account: account.presence, onSignIn: account.signIn, signingIn: account.signingIn, cancelSignIn: account.cancelSignIn, onChanged, enabled });
  const { view, snapshot, loadFailed, listFailure, failures, busy, signingIn, openPlans, dismissFailure, signIn, load, rename, renameSelf, revoke, revokeSession, retryEnrollment } = machines;
  const lastRefreshKey = useRef(registryRefreshKey);
  useEffect(() => {
    if (lastRefreshKey.current === registryRefreshKey) return;
    lastRefreshKey.current = registryRefreshKey;
    if (enabled) void load();
  }, [registryRefreshKey, load, enabled]);
  const canvas = useMemo(
    () => accountCanvas({ view, peers, loadFailed, listFailed: listFailure !== null, failures }),
    [view, peers, loadFailed, listFailure, failures],
  );
  // Ages are read against the moment the facts behind them last changed.
  const now = useMemo(() => Date.now(), [snapshot, peers]);
  const actions = useMemo(() => ({
    onSignOut: revokeSession,
    onRemove: revoke,
    onRename: rename,
    onPlans: openPlans,
    onDismiss: dismissFailure,
  }), [openPlans, dismissFailure, rename, revoke, revokeSession]);
  const self = view.kind === "account" ? view.self : null;
  return useMemo(() => ({
    selfSameNameOthers: view.kind === "account" ? view.selfSameNameOthers : undefined,
    moreMachines: !!snapshot?.registry?.cursor,
    canvas, busy, signingIn, cancelSignIn: account.cancelSignIn, now, actions, selfName: view.kind === "account" ? view.registrySelfName ?? (self?.displayName ? self.name : undefined) : undefined, selfDisplayName: self?.displayName, onRenameSelf: self && !self.readOnly ? renameSelf : undefined, removed: view.kind === "account" && view.removed, onRejoin: () => { void signIn(); }, reauthRequired: view.kind === "account" && view.reauthRequired,
    limitMachines: view.kind === "account" && view.machineLimit ? machines : undefined,
    onSignIn: () => { void signIn(); },
    onRetry: () => { void load(); },
    onRetryEnrollment: account.presence === "signed_in" && snapshot?.enrollment === "refused" && snapshot.retry_at == null
      && ["unavailable", "offline", "authority-unavailable", "enrollment_budget"].includes(snapshot.last_refusal?.reason ?? "")
      ? () => { void retryEnrollment(); } : undefined,
  }), [canvas, busy, signingIn, now, actions, self, renameSelf, view, signIn, load, snapshot, machines, account.presence, account.cancelSignIn, retryEnrollment]);
}
