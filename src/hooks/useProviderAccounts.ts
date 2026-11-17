import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "../lib/bridge";
import { selectedDaemonTransport } from "../lib/daemonRuntime";
import type { ProviderAccountsAction, ProviderAccountsSnapshot } from "./providerAccountsTypes";

interface AccountResource {
  providerId: string;
  active: boolean;
  sequence: number;
  reading: AbortController | null;
  mutating: boolean;
  refreshPending: boolean;
  snapshot: ProviderAccountsSnapshot | null;
}

function acceptSnapshot(provider: string, snapshot: ProviderAccountsSnapshot): ProviderAccountsSnapshot {
  if (snapshot.providerId !== provider || typeof snapshot.revision !== "string" || !Array.isArray(snapshot.accounts)) {
    throw new Error("The account response does not match this harness");
  }
  return snapshot;
}

export function useProviderAccounts(providerId: string | null) {
  const resource = useRef<AccountResource | null>(null);
  const [view, setView] = useState<{
    providerId: string | null;
    snapshot: ProviderAccountsSnapshot | null;
    loading: boolean;
    error: string | null;
  }>({ providerId, snapshot: null, loading: Boolean(providerId), error: null });

  const read = useCallback(async (current: AccountResource) => {
    if (!current.active) return;
    if (current.mutating || current.reading) {
      current.refreshPending = true;
      return;
    }
    const controller = new AbortController();
    current.reading = controller;
    current.refreshPending = false;
    const sequence = ++current.sequence;
    try {
      const response = await selectedDaemonTransport.json<ProviderAccountsSnapshot>(
        "GET", `/api/agents/providers/${encodeURIComponent(current.providerId)}/accounts`, undefined,
        { signal: controller.signal },
      );
      if (!current.active || sequence !== current.sequence) return;
      current.snapshot = acceptSnapshot(current.providerId, response);
      setView({ providerId: current.providerId, snapshot: current.snapshot, loading: false, error: null });
    } catch (error) {
      if (current.active && sequence === current.sequence && !controller.signal.aborted) {
        setView({ providerId: current.providerId, snapshot: current.snapshot, loading: false, error: String(error) });
      }
    } finally {
      if (current.reading === controller) current.reading = null;
      if (current.active && current.refreshPending && !current.mutating) void read(current);
    }
  }, []);

  useEffect(() => {
    setView({ providerId, snapshot: null, loading: Boolean(providerId), error: null });
    if (!providerId) { resource.current = null; return; }
    const current: AccountResource = {
      providerId, active: true, sequence: 0, reading: null, mutating: false, refreshPending: false, snapshot: null,
    };
    resource.current = current;
    let stop: UnlistenFn | undefined;
    void listen<{ providerId: string }>("provider-accounts-changed", ({ payload }) => {
      if (current.active && payload.providerId === current.providerId) void read(current);
    }).then(unlisten => {
      if (!current.active) { unlisten(); return; }
      stop = unlisten;
      void read(current);
    }).catch(error => {
      if (current.active) setView({ providerId, snapshot: null, loading: false, error: String(error) });
    });
    return () => {
      current.active = false;
      current.sequence++;
      current.reading?.abort();
      stop?.();
      if (resource.current === current) resource.current = null;
    };
  }, [providerId, read]);

  const onRefresh = useCallback(() => {
    const current = resource.current;
    if (current?.providerId === providerId) void read(current);
  }, [providerId, read]);

  const onAction = useCallback(async (action: ProviderAccountsAction): Promise<void> => {
    const current = resource.current;
    const rendered = view.providerId === providerId ? view.snapshot : null;
    if (!current?.active || current.providerId !== providerId || !current.snapshot) {
      throw new Error("The account list is unavailable");
    }
    if (current.mutating) throw new Error("Another account change is in progress");
    if (!rendered || rendered.revision !== current.snapshot.revision || rendered.signInId !== current.snapshot.signInId) {
      const error = new Error("The account list changed. Refresh it before trying again.");
      setView({providerId:current.providerId,snapshot:current.snapshot,loading:false,error:error.message});
      throw error;
    }
    current.mutating = true;
    current.sequence++;
    current.reading?.abort();
    current.reading = null;
    try {
      const response = await selectedDaemonTransport.json<ProviderAccountsSnapshot>(
        "POST", `/api/agents/providers/${encodeURIComponent(current.providerId)}/accounts`,
        { revision: rendered.revision, ...(rendered.signInId ? {signInId:rendered.signInId} : {}), operation: action },
      );
      if (!current.active) throw new Error("The harness changed before the account response arrived");
      current.snapshot = acceptSnapshot(current.providerId, response);
      setView({ providerId: current.providerId, snapshot: current.snapshot, loading: false, error: null });
    } catch (error) {
      if (current.active) setView({ providerId: current.providerId, snapshot: current.snapshot, loading: false, error: String(error) });
      throw error;
    } finally {
      current.mutating = false;
      if (current.active && current.refreshPending) void read(current);
    }
  }, [providerId, read, view.providerId, view.snapshot]);

  const own = view.providerId === providerId;
  return {
    snapshot: own ? view.snapshot : null,
    loading: own ? view.loading : Boolean(providerId),
    error: own ? view.error : null,
    onAction, onRefresh,
  };
}
