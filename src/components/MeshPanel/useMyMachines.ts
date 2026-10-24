import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { openLockedMachinePlans } from "../billing/accountMeshLimit";
import { myMachinesView, type AccountPresence, type MachineRowView, type MyMachinesView } from "./accountMachines";
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
  revoke: (row: MachineRowView, target: MeshTarget) => Promise<boolean>;
  renameSelf: (name: string) => Promise<boolean>;
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
      setSnapshot(next);
      setLoadFailed(false);
    } catch {
      if (current === generation.current) setLoadFailed(true);
    } finally {
      if (current === generation.current) setRefreshing(false);
    }
  }, [account, client]);

  useEffect(() => {
    void load();
    return client.subscribe(() => { void load(false); });
  }, [client, load, account]);

  const fail = useCallback((key: string, operation: string, failure: AccountMeshFailure) => {
    setFailures((current) => new Map(current).set(key, { operation, failure }));
    onFailure?.(operation, failure);
  }, [onFailure]);

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

  const revoke = useCallback(async (row: MachineRowView, target: MeshTarget) => {
    setBusy(row.key);
    const outcome = await settle(() => client.revoke(target));
    setBusy(null);
    if (!outcome.ok) {
      fail(row.key, `revoke ${row.name}`, outcome.failure);
      return false;
    }
    onChanged?.();
    await load();
    return true;
  }, [client, fail, load, onChanged]);

  const renameSelf = useCallback(async (name: string) => {
    const self = view.kind === "account" ? view.self : null;
    if (!self) return false;
    const outcome = await settle(() => client.renameSelf(self.metadataRevision, name));
    if (!outcome.ok) {
      onFailure?.("rename this machine", outcome.failure);
      return false;
    }
    await load();
    return true;
  }, [client, load, onFailure, view]);

  const retryEnrollment = useCallback(async () => {
    const result = await settle(() => client.enroll());
    if (!result.ok) onFailure?.("retry account enrollment", result.failure);
    await load(false);
  }, [client, load, onFailure]);

  const openPlans = useCallback((key: string) => openLockedMachinePlans(snapshot, key), [snapshot]);

  return {
    view, snapshot, listFailure, loadFailed, refreshing, signingIn, busy: deciding ?? busy, failures,
    load, signIn, connect, decide, disconnect, revoke, renameSelf, retryEnrollment, openPlans, dismissFailure,
  };
}
