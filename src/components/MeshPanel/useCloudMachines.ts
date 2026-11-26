import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import {
  CloudApiError,
  createCloudMachine,
  deleteCloudMachine,
  getCloudUsage,
  listCloudMachines,
  redeemCloudPromo,
  sleepCloudMachine,
  wakeCloudMachine,
} from "../../lib/cloudMachines/client";
import {
  cloudFailurePresentation,
  type CloudFailurePresentation,
} from "../../lib/cloudMachines/stateModel";
import type {
  CloudPromoRedemption,
  CloudReceipt,
  CloudUsagePayload,
} from "../../lib/cloudMachines/types";

export interface PendingCloudCreate {
  size: string;
  status: "creating" | "failed";
  failure?: CloudFailurePresentation;
}

export function useCloudUsage(enabled = true) {
  const [usage, setUsage] = useState<CloudUsagePayload | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);

  const refresh = useCallback(async () => {
    if (!enabled) return null;
    const current = ++requestId.current;
    setLoading(true);
    try {
      const next = await getCloudUsage();
      if (current === requestId.current) {
        setUsage(next);
        setError(null);
      }
      return next;
    } catch (err) {
      if (current === requestId.current) {
        setError(err instanceof Error ? err.message : "Could not load cloud hours");
      }
      return null;
    } finally {
      if (current === requestId.current) setLoading(false);
    }
  }, [enabled]);

  useEffect(() => {
    if (!enabled) return;
    let active = true;
    const unlisteners: Array<() => void> = [];
    void refresh();
    const onFocus = () => { if (active) void refresh(); };
    window.addEventListener("focus", onFocus);
    for (const event of ["mesh-state-changed", "entitlement-changed", "account-changed"]) {
      void listen(event, () => { if (active) void refresh(); })
        .then((unlisten) => { if (active) unlisteners.push(unlisten); else unlisten(); })
        .catch(() => undefined);
    }
    return () => {
      active = false;
      window.removeEventListener("focus", onFocus);
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [enabled, refresh]);

  const applyRedemption = useCallback((redemption: CloudPromoRedemption) => {
    setUsage((current) => current
      ? { ...current, remaining: redemption.remaining, purchased: redemption.purchased }
      : current);
  }, []);

  return { usage, loading, error, refresh, applyRedemption };
}

export function useCloudMachines(enabled: boolean, onMeshChanged?: () => void) {
  const [machines, setMachines] = useState<CloudReceipt[]>([]);
  const [pendingCreate, setPendingCreate] = useState<PendingCloudCreate | null>(null);
  const [busyMachineId, setBusyMachineId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const usageState = useCloudUsage(enabled);
  const listReqId = useRef(0);

  const refreshMachines = useCallback(async () => {
    if (!enabled) return [];
    const current = ++listReqId.current;
    try {
      const next = await listCloudMachines();
      if (current === listReqId.current) {
        setMachines(next.filter((machine) => machine.state !== "deleted"));
      }
      return next;
    } catch {
      return [];
    }
  }, [enabled]);

  useEffect(() => {
    if (!enabled) return;
    let active = true;
    const unlisteners: Array<() => void> = [];
    void refreshMachines();
    for (const event of ["mesh-state-changed", "account-mesh-changed"]) {
      void listen(event, () => { if (active) void refreshMachines(); })
        .then((unlisten) => { if (active) unlisteners.push(unlisten); else unlisten(); })
        .catch(() => undefined);
    }
    return () => {
      active = false;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [enabled, refreshMachines]);

  const create = useCallback(async (size: string) => {
    setActionError(null);
    setPendingCreate({ size, status: "creating" });
    try {
      const receipt = await createCloudMachine({ size });
      setPendingCreate(null);
      setMachines((current) => [
        ...current.filter((item) => item.machine_id !== receipt.machine_id),
        receipt,
      ]);
      onMeshChanged?.();
      void usageState.refresh();
      return { ok: true as const, receipt };
    } catch (err) {
      const code = err instanceof CloudApiError ? err.code : undefined;
      const failure = cloudFailurePresentation(code);
      setPendingCreate({ size, status: "failed", failure });
      return { ok: false as const, code, failure };
    }
  }, [onMeshChanged, usageState]);

  const clearPendingCreate = useCallback(() => setPendingCreate(null), []);

  const sleep = useCallback(async (machineId: string) => {
    setBusyMachineId(machineId);
    setActionError(null);
    try {
      const receipt = await sleepCloudMachine(machineId);
      setMachines((current) => current.map((item) => (item.machine_id === receipt.machine_id ? receipt : item)));
      onMeshChanged?.();
      void usageState.refresh();
      return { ok: true as const, receipt };
    } catch (err) {
      const message = err instanceof CloudApiError ? err.message : "Couldn't put this machine to sleep.";
      setActionError(message);
      return { ok: false as const, message };
    } finally {
      setBusyMachineId(null);
    }
  }, [onMeshChanged, usageState]);

  const wake = useCallback(async (machineId: string) => {
    setBusyMachineId(machineId);
    setActionError(null);
    setMachines((current) => current.map((item) => (item.machine_id === machineId ? { ...item, state: "waking" } : item)));
    try {
      const receipt = await wakeCloudMachine(machineId);
      setMachines((current) => current.map((item) => (item.machine_id === receipt.machine_id ? receipt : item)));
      onMeshChanged?.();
      void usageState.refresh();
      return { ok: true as const, receipt };
    } catch (err) {
      const code = err instanceof CloudApiError ? err.code : undefined;
      if (code === "credits_exhausted" || code === "lease_required") {
        setMachines((current) => current.map((item) => (item.machine_id === machineId ? { ...item, state: "out_of_credits" } : item)));
      } else {
        void refreshMachines();
      }
      const message = err instanceof CloudApiError ? err.message : "Couldn't wake this machine.";
      setActionError(message);
      return { ok: false as const, code, message };
    } finally {
      setBusyMachineId(null);
    }
  }, [onMeshChanged, refreshMachines, usageState]);

  const remove = useCallback(async (machineId: string) => {
    setBusyMachineId(machineId);
    setActionError(null);
    try {
      await deleteCloudMachine(machineId);
      setMachines((current) => current.filter((item) => item.machine_id !== machineId));
      onMeshChanged?.();
      void usageState.refresh();
      return { ok: true as const };
    } catch (err) {
      const message = err instanceof CloudApiError ? err.message : "Couldn't delete this machine.";
      setActionError(message);
      return { ok: false as const, message };
    } finally {
      setBusyMachineId(null);
    }
  }, [onMeshChanged, usageState]);

  const redeem = useCallback(async (code: string) => {
    const result = await redeemCloudPromo(code);
    usageState.applyRedemption(result);
    void usageState.refresh();
    return result;
  }, [usageState]);

  const refreshAll = useCallback(async () => {
    await Promise.all([refreshMachines(), usageState.refresh()]);
  }, [refreshMachines, usageState]);

  return {
    machines,
    usage: usageState.usage,
    usageLoading: usageState.loading,
    usageError: usageState.error,
    pendingCreate,
    busyMachineId,
    actionError,
    refreshAll,
    refreshUsage: usageState.refresh,
    create,
    clearPendingCreate,
    sleep,
    wake,
    remove,
    redeem,
  };
}
