import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { openLockedMachinePlans } from "../billing/accountMeshLimit";
import { failureMessage, myMachinesView, type AccountPresence, type MachineRowView, type MyMachinesView } from "./accountMachines";
import {
  useAccountMeshClient,
  type AccountMeshFailure,
  type AccountMeshOutcome,
  type AccountMeshSnapshot,
  type MeshTarget,
} from "./accountMeshClient";

import { useMachineLinkActions } from "./useMachineLinkActions";

export interface MachineFailure {
  operation: string;
  failure: AccountMeshFailure;
  retryAt?: number;
}

export interface MyMachinesOptions {
  account: AccountPresence;
  onSignIn: () => void | Promise<void>;
  /** Also reported per machine through `failures`. */
  onFailure?: (operation: string, failure: AccountMeshFailure) => void;
  onChanged?: () => void;
  now?: () => number;
}

export interface MyMachines {
  view: MyMachinesView;
  snapshot: AccountMeshSnapshot | null;
  listFailure: AccountMeshFailure | null;
  loadFailed: boolean;
  refreshing: boolean;
  signingIn: boolean;
  busy: string | null;
  failures: ReadonlyMap<string, MachineFailure>;
  load: () => Promise<void>;
  signIn: () => Promise<void>;
  connect: (row: MachineRowView, target: MeshTarget) => Promise<void>;
  decide: (row: MachineRowView, notificationId: string, approve: boolean) => Promise<void>;
  disconnect: (row: MachineRowView, instanceId: string) => Promise<boolean>;
  revoke: (row: MachineRowView, target: MeshTarget, alsoSignOut?: boolean) => Promise<boolean>;
  revokeSession: (row: MachineRowView) => Promise<boolean>;
  renameSelf: (name: string) => Promise<boolean>;
  rename: (row: MachineRowView, name: string) => Promise<boolean>;
  retryEnrollment: () => Promise<void>;
  openPlans: (key: string) => void;
  dismissFailure: (key: string) => void;
}

const epochSeconds = () => Math.floor(Date.now() / 1000);

async function settle(run: () => Promise<AccountMeshOutcome>): Promise<AccountMeshOutcome> {
  try {
    return await run();
  } catch {
    return { ok: false, failure: { reason: "unavailable" } };
  }
}

/** Account machines with their live snapshot and the actions that change them, shared by the canvas and the list. */
export function useMyMachines({ account, onSignIn, onFailure, onChanged, now = epochSeconds }: MyMachinesOptions): MyMachines {
  const client = useAccountMeshClient();
  const [snapshot, setSnapshot] = useState<AccountMeshSnapshot | null>(null);
  const [listFailure, setListFailure] = useState<AccountMeshFailure | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [signingIn, setSigningIn] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [failures, setFailures] = useState<ReadonlyMap<string, MachineFailure>>(() => new Map());
  const generation = useRef(0);
  const refusedListEpoch = useRef<number | null>(null);
  const failureEpoch = useRef<number | null>(null);
  useEffect(() => {
    if (!snapshot) return;
    const changedEpoch = failureEpoch.current !== snapshot.account_epoch;
    failureEpoch.current = snapshot.account_epoch;
    setFailures(current => {
      const next = changedEpoch ? new Map<string, MachineFailure>() : new Map(current);
      for (const [key, failure] of next) {
        if (failure.operation.startsWith("connect ") && snapshot.pairs.some(pair => pair.installation_id === key)) next.delete(key);
      }
      return next.size === current.size && [...next.keys()].every(key => current.has(key)) ? current : next;
    });
  }, [snapshot]);

  const load = useCallback(async (refreshRegistry = true) => {
    const current = ++generation.current;
    setRefreshing(true);
    try {
      if (account === "signed_in" && refreshRegistry) {
        const result = await settle(() => client.list());
        if (current !== generation.current) return;
        setListFailure(result.ok ? null : result.failure);
      }
      const next = await client.snapshot();
      if (current !== generation.current) return;
      if (!refreshRegistry) setListFailure(next.registry_failure);
      setSnapshot((previous) => ({ ...next, registry: next.registry && { ...next.registry,
        rows: next.registry.rows.map((row) => {
          const known = previous?.account_epoch === next.account_epoch
            ? previous?.registry?.rows.find((candidate) => candidate.installation_id === row.installation_id) : undefined;
          return known && known.key_id === row.key_id && known.key_generation === row.key_generation
            && known.slot_revision === row.slot_revision && known.metadata_revision > row.metadata_revision
            ? { ...row, display_name: known.display_name, metadata_revision: known.metadata_revision } : row;
        }),
      } }));
      setLoadFailed(false);
    } catch {
      if (current === generation.current) setLoadFailed(true);
    } finally {
      if (current === generation.current) setRefreshing(false);
    }
  }, [account, client]);

  useEffect(() => {
    void load(false);
    return client.subscribe(() => { void load(false); });
  }, [client, load, account]);

  useEffect(() => {
    if (account !== "signed_in" || snapshot?.enrollment !== "refused" || snapshot.last_refusal?.reason !== "mesh.machines"
      || refusedListEpoch.current === snapshot.account_epoch) return;
    refusedListEpoch.current = snapshot.account_epoch;
    void load();
  }, [account, snapshot, load]);

  const fail = useCallback((key: string, operation: string, failure: AccountMeshFailure) => {
    setFailures((current) => new Map(current).set(key, { operation, failure, ...(failure.reason === "rate-limited" ? { retryAt: now() + failure.retry_after_seconds } : {}) }));
    onFailure?.(operation, failure);
  }, [onFailure, now]);

  const dismissFailure = useCallback((key: string) => {
    setFailures((current) => {
      if (!current.has(key)) return current;
      const next = new Map(current);
      next.delete(key);
      return next;
    });
  }, []);

  const signIn = useCallback(async () => {
    setSigningIn(true);
    try {
      await onSignIn();
    } finally {
      setSigningIn(false);
    }
  }, [onSignIn]);

  const { linking, connect, decide, deciding } = useMachineLinkActions({ client, snapshot, load, onChanged, fail, dismissFailure });
  const view = useMemo(() => myMachinesView({ account, snapshot, listFailure, linking, now: now() }), [account, snapshot, listFailure, linking, now]);

  const disconnect = useCallback(async (row: MachineRowView, instanceId: string) => {
    dismissFailure(row.key);
    setBusy(row.key);
    try {
      await client.unpair(instanceId);
    } catch {
      setBusy(null);
      fail(row.key, `disconnect ${row.name}`, { reason: "unavailable" });
      return false;
    }
    setBusy(null);
    onChanged?.();
    await load();
    return true;
  }, [client, dismissFailure, fail, load, onChanged]);

  const revoke = useCallback(async (row: MachineRowView, target: MeshTarget, alsoSignOut = false) => {
    setBusy(row.key);
    dismissFailure(row.key);
    if (alsoSignOut) {
      const signedOut = await settle(() => client.revokeSession(target.installation_id));
      if (!signedOut.ok) {
        setBusy(null);
        fail(row.key, `sign out ${row.name}`, signedOut.failure);
        return false;
      }
    }
    const outcome = await settle(() => client.revoke(target));
    setBusy(null);
    if (!outcome.ok) {
      fail(row.key, `revoke ${row.name}`, outcome.failure);
      return false;
    }
    onChanged?.();
    await load();
    return true;
  }, [client, dismissFailure, fail, load, onChanged]);

  const revokeSession = useCallback(async (row: MachineRowView) => {
    dismissFailure(row.key);
    setBusy(row.key);
    const outcome = await settle(() => client.revokeSession(row.key));
    setBusy(null);
    if (!outcome.ok) {
      fail(row.key, `sign out ${row.name}`, outcome.failure);
      return false;
    }
    onChanged?.();
    await load();
    return true;
  }, [client, dismissFailure, fail, load, onChanged]);

  const renameSelf = useCallback(async (name: string) => {
    const self = (await client.snapshot()).self;
    if (!self) return false;
    const outcome = await settle(() => client.renameSelf(self.metadata_revision, name));
    if (!outcome.ok) {
      onFailure?.("rename this machine", outcome.failure);
      throw new Error(failureMessage(outcome.failure));
    }
    await load(false);
    return true;
  }, [client, load, onFailure]);

  const retryEnrollment = useCallback(async () => {
    dismissFailure("enrollment");
    setBusy("enrollment");
    const result = await settle(() => client.enroll());
    setBusy(null);
    if (!result.ok) fail("enrollment", "join mesh", result.failure);
    await load(false);
  }, [client, load, fail, dismissFailure]);

  const rename = useCallback(async (row: MachineRowView, name: string) => {
    const registry = snapshot?.registry?.rows.find((candidate) => candidate.installation_id === row.key);
    const instanceId = row.pairInstanceId;
    const renamePeer = client.renamePeer;
    if (!registry || !instanceId || !row.online || !renamePeer) return false;
    dismissFailure(row.key);
    const outcome = await settle(() => renamePeer.call(client, instanceId, registry.metadata_revision, name));
    if (!outcome.ok) { fail(row.key, `rename ${row.name}`, outcome.failure); throw new Error(failureMessage(outcome.failure)); }
    const updated = outcome.result;
    if (updated) setSnapshot((current) => current && current.account_epoch === snapshot?.account_epoch ? ({ ...current, registry: current.registry && {
      ...current.registry, rows: current.registry.rows.map((candidate) => candidate.installation_id === row.key ? { ...candidate, ...updated } : candidate),
    } }) : current);
    onChanged?.();
    return true;
  }, [client, dismissFailure, fail, onChanged, snapshot]);

  const openPlans = useCallback((key: string) => openLockedMachinePlans(snapshot, key), [snapshot]);

  return {
    view, snapshot, listFailure, loadFailed, refreshing, signingIn, busy: deciding ?? busy, failures,
    load, signIn, connect, decide, disconnect, revoke, revokeSession, renameSelf, rename, retryEnrollment, openPlans, dismissFailure,
  };
}
