import { useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { SharedRead } from "../../lib/sharedRead";
import { receiverState, type ChannelStatus, type NeighborRuntimeClient } from "./neighborRuntimeClient";

/** The runtime status every Neighbors surface shares: one read, then events keep it current. */
export type NeighborStatusSnapshot = ChannelStatus & { concierges?: Record<string, unknown>; receiver_status_known?: boolean };

let stores = new WeakMap<NeighborRuntimeClient, SharedRead<NeighborStatusSnapshot>>();

function storeFor(client: NeighborRuntimeClient): SharedRead<NeighborStatusSnapshot> {
  let store = stores.get(client);
  if (!store) {
    store = new SharedRead<NeighborStatusSnapshot>();
    stores.set(client, store);
  }
  return store;
}

/** Forget every snapshot; the status belongs to one account and must not outlive a sign-out or switch. */
export function resetNeighborStatus() {
  stores = new WeakMap();
}

/**
 * The shared snapshot while `active`. Events write into it, and a write supersedes any read still in flight,
 * so a late initial answer can never restore an older state; turning inactive drops it from this caller.
 */
export function useNeighborStatus(client: NeighborRuntimeClient, active: boolean): NeighborStatusSnapshot | null {
  const [status, setStatus] = useState<NeighborStatusSnapshot | null>(null);
  const clientRef = useRef(client);
  clientRef.current = client;
  useEffect(() => {
    if (!active) { setStatus(null); return; }
    const client = clientRef.current;
    const store = storeFor(client);
    let live = true;
    const show = (value: NeighborStatusSnapshot) => { if (live) setStatus(value); };
    const load = () => client.status() as Promise<NeighborStatusSnapshot>;
    const stops: Array<() => void> = [];
    const keep = (stop: () => void) => { if (live) stops.push(stop); else stop(); };
    stops.push(store.subscribe(show));
    if (store.value) setStatus(store.value);
    store.read(load).then(show, () => {});
    // An event before the first answer cannot patch a snapshot yet, so it asks for a read that starts after it.
    const reread = () => { store.read(load, true).then(show, () => {}); };
    void client.subscribe(event => {
      if (!store.value) { reread(); return; }
      if ("receiver_unavailable" in event) { store.write({ ...store.value, receiver_status_known: false }); return; }
      if (!receiverState(event)) return;
      const { receiving_here, receiver_machine_label, receiving_enabled, revision } = event;
      store.write({ ...store.value, receiving_here, receiver_machine_label, receiving_enabled, revision, receiver_status_known: true });
      // Moving the receiver changes every concierge's state too, which only a fresh read reports.
      reread();
    }).then(keep);
    void listen<unknown>("neighbor-concierge-status", event => {
      const payload = event.payload as { friendship_id?: unknown } | null;
      if (!payload || typeof payload.friendship_id !== "string") return;
      if (!store.value) { reread(); return; }
      store.write({ ...store.value, concierges: { ...(store.value.concierges ?? {}), [payload.friendship_id]: payload } });
    }).then(keep);
    // A new or replaced concierge changes bindings for every surface at once; read it again.
    void listen("neighbor-agent-changed", reread).then(keep);
    return () => { live = false; stops.forEach(stop => stop()); };
  }, [active]);
  return status;
}
