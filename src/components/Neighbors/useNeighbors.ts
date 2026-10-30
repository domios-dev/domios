import { useEffect, useRef, useState } from "react";
import { neighborClient, type NeighborClient } from "./neighborClient";
import type { NeighborCommand, NeighborList } from "./neighborModel";
import { neighborReads } from "./neighborReads";

interface LoadedNeighbors {
  identity: string | null;
  list: NeighborList | null;
  error: string | null;
}

export function useNeighbors(identity: string | null, client: NeighborClient = neighborClient) {
  const [loaded, setLoaded] = useState<LoadedNeighbors>({ identity, list: null, error: null });
  const [pending, setPending] = useState(false);
  const busy = useRef(false);
  const currentIdentity = useRef(identity);
  currentIdentity.current = identity;

  useEffect(() => {
    const shared = neighborReads(client, identity);
    if (!shared) { setLoaded({ identity, list: null, error: null }); return; }
    let live = true;
    const show = (list: NeighborList) => { if (live) setLoaded({ identity, list, error: null }); };
    const showRead = (read: Promise<NeighborList>) => {
      void read.then(show, (failure: unknown) => {
        if (live) setLoaded(previous => ({ identity,
          list: previous.identity === identity && previous.list ? { ...previous.list, can_invite: false } : null,
          error: failure instanceof Error ? failure.message : "Neighbors unavailable" }));
      });
    };
    const unsubscribe = shared.list.subscribe(show);
    const stopReloads = shared.subscribeReloads(client, showRead);
    if (shared.list.value) show(shared.list.value);
    showRead(shared.list.read(() => client.list()));
    return () => { live = false; unsubscribe(); stopReloads(); };
  }, [identity, client]);

  async function refresh() {
    const shared = neighborReads(client, identity);
    if (!shared) return;
    try { await shared.list.read(() => client.list(undefined, true), true); }
    catch (failure: unknown) {
      if (currentIdentity.current === identity) setLoaded(previous => ({ ...previous,
        error: failure instanceof Error ? failure.message : "Neighbors unavailable" }));
    }
  }

  async function apply(command: Exclude<NeighborCommand, { operation: "list" }>): Promise<boolean> {
    if (busy.current || !identity) return false;
    busy.current = true;
    setPending(true);
    try {
      await client.apply(command);
      if (currentIdentity.current !== identity) return false;
      await refresh();
      return true;
    } catch (failure: unknown) {
      if (currentIdentity.current === identity) setLoaded(previous => ({ ...previous,
        list: command.operation === "request" && previous.list ? { ...previous.list, can_invite: false } : previous.list,
        error: failure instanceof Error ? failure.message : "Neighbors request failed" }));
      return false;
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  const list = loaded.identity === identity ? loaded.list : null;
  return { list, referral: list?.referral ?? null,
    error: loaded.identity === identity ? loaded.error : null, pending, apply, refresh };
}
