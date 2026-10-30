import { useEffect, useState, useSyncExternalStore } from "react";
import { listen } from "../../lib/bridge";
import { SharedRead } from "../../lib/sharedRead";
import { receiverState, type ChannelStatus, type NeighborRuntimeClient } from "./neighborRuntimeClient";

/** The runtime status every Neighbors surface shares: one read, then events keep it current. */
export type NeighborStatusSnapshot = ChannelStatus & { concierges?: Record<string, unknown> };

let stores = new WeakMap<NeighborRuntimeClient, StatusStore>();
const activeStores = new Set<StatusStore>();
let statusGeneration = 0;
const resetListeners = new Set<() => void>();
const subscribeReset = (notify: () => void) => { resetListeners.add(notify); return () => { resetListeners.delete(notify); }; };

export function resetNeighborStatus() {
  for (const store of activeStores) store.stop();
  activeStores.clear();
  stores = new WeakMap();
  statusGeneration++;
  queueMicrotask(() => { for (const notify of resetListeners) notify(); });
}

/** Receiver fields only move forward: a read answered before a newer receiver event keeps that event's state. */
function keepNewerReceiver(fresh: NeighborStatusSnapshot, current: NeighborStatusSnapshot | undefined): NeighborStatusSnapshot {
  if (!current) return fresh;
  let next = fresh;
  if (current.friend_receivers_version !== undefined && current.friend_receivers_version > (fresh.friend_receivers_version ?? -1)) {
    next = { ...next, friend_receivers: current.friend_receivers, friend_receivers_version: current.friend_receivers_version };
  }
  if (fresh.friend_receivers_version === undefined || current.friend_receivers_version === undefined
    || fresh.friend_receivers_version === current.friend_receivers_version) {
    const receivers = { ...next.friend_receivers };
    for (const [id, receiver] of Object.entries(current.friend_receivers ?? {})) {
      const observed = receivers[id];
      if (observed && receiver.revision >= observed.revision) receivers[id] = receiver;
    }
    next = { ...next, friend_receivers: receivers };
  }
  return next;
}

class StatusStore extends SharedRead<NeighborStatusSnapshot> {
  private live = true;
  private readonly stops: Array<() => void> = [];

  constructor(client: NeighborRuntimeClient) {
    super(undefined, Infinity);
    const load = () => (client.status() as Promise<NeighborStatusSnapshot>).then(fresh => keepNewerReceiver(fresh, this.value));
    const keep = (stop: () => void) => { if (this.live) this.stops.push(stop); else stop(); };
    const reread = () => { if (this.live) void this.read(load, true).catch(() => {}); };
    void client.subscribe(event => {
      if (!this.live) return;
      const current = this.value;
      if (!current) { reread(); return; }
      if ("receiver_unavailable" in event) {
        if (event.friend_receivers_version !== undefined && event.friend_receivers_version < (current.friend_receivers_version ?? -1)) return;
        const receivers = typeof event.friendship_id === "string" ? { ...current.friend_receivers } : {};
        if (typeof event.friendship_id === "string") delete receivers[event.friendship_id];
        this.write({ ...current, friend_receivers: receivers, friend_receivers_version: event.friend_receivers_version ?? current.friend_receivers_version });
        return;
      }
      if (!receiverState(event) || typeof event.friendship_id !== "string") return;
      if (event.friend_receivers_version !== undefined && event.friend_receivers_version < (current.friend_receivers_version ?? -1)) return;
      const { receiving_here, receiver_machine_label, receiving_enabled, revision, is_me } = event;
      this.write({ ...current, friend_receivers_version: event.friend_receivers_version ?? current.friend_receivers_version,
        friend_receivers: { ...current.friend_receivers,
          [event.friendship_id]: { receiving_here, receiver_machine_label, receiving_enabled, revision, is_me } } });
      reread();
    }).then(keep);
    void listen<unknown>("neighbor-concierge-status", event => {
      if (!this.live) return;
      const payload = event.payload as { friendship_id?: unknown } | null;
      if (!payload || typeof payload.friendship_id !== "string") return;
      if (!this.value) { reread(); return; }
      this.write({ ...this.value, concierges: { ...(this.value.concierges ?? {}), [payload.friendship_id]: payload } });
    }).then(keep);
    void listen("neighbor-agent-changed", reread).then(keep);
    void this.read(load).catch(() => {});
  }

  stop() { this.live = false; this.stops.forEach(stop => stop()); }
}

export function useNeighborStatus(client: NeighborRuntimeClient, active: boolean): NeighborStatusSnapshot | null {
  const generation = useSyncExternalStore(subscribeReset, () => statusGeneration);
  const [status, setStatus] = useState<NeighborStatusSnapshot | null>(null);
  useEffect(() => {
    if (!active) { setStatus(null); return; }
    let store = stores.get(client);
    if (!store) { store = new StatusStore(client); stores.set(client, store); activeStores.add(store); }
    setStatus(store.value ?? null);
    return store.subscribe(setStatus);
  }, [active, client, generation]);
  return status;
}
