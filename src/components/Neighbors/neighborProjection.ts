import { useEffect, useState, useSyncExternalStore } from "react";
import { relayTarget, webJson } from "../../lib/webApi";
import { publishNeighborGateways } from "./neighborGateways";
import { resetNeighborStatus } from "./neighborStatusStore";
import { resetPausedFriends } from "./neighborPause";
import { resetNeighborReads } from "./neighborReads";
import { listen } from "../../lib/bridge";
import { onResync } from "../../lib/resync";
import { receiverState, type ReceiverState } from "./neighborRuntimeClient";
import { observeNeighborStatus, type NeighborStatusObservation } from "./neighborStatusObservations";

export type NeighborProjection = Record<string, {
  tab_id: string | null;
  friend_display_name: string;
  friend_photo_url: string | null;
  paused: boolean;
  access: "paid" | "locked" | null;
  receiver: ReceiverState | null;
  concierge?: unknown;
  observation?: NeighborStatusObservation;
  status_stale?: boolean;
}>;

export function observeNeighborProjection(fresh: NeighborProjection, current: NeighborProjection = {}, stale = false): NeighborProjection {
  return Object.fromEntries(Object.entries(fresh).map(([id, entry]) => {
    const previous = current[id];
    const observed = observeNeighborStatus({ bindings: { [id]: entry.tab_id ?? "" },
      friend_receivers: entry.receiver ? { [id]: entry.receiver } : {}, concierges: { [id]: entry.concierge }, status_stale: stale || entry.status_stale,
    }, previous ? {
      bindings: { [id]: previous.tab_id ?? "" }, observations: previous.observation ? { [id]: previous.observation } : {},
    } : undefined);
    return [id, { ...entry, observation: observed.observations?.[id] }];
  }));
}

export function projectionForPeer(selected: string | null, source: string | null, projection: NeighborProjection): NeighborProjection {
  return selected === source ? projection : {};
}

function subscribePeer(notify: () => void) {
  const reset = () => {
    publishNeighborGateways({});
    resetNeighborStatus();
    resetPausedFriends();
    resetNeighborReads();
    notify();
  };
  window.addEventListener("codeterm-peer-switch-reset", reset);
  window.addEventListener("codeterm-peer-switch-rebind", notify);
  return () => {
    window.removeEventListener("codeterm-peer-switch-reset", reset);
    window.removeEventListener("codeterm-peer-switch-rebind", notify);
  };
}

export function useNeighborPeer(): string | null {
  return useSyncExternalStore(subscribePeer, () => relayTarget()?.id ?? null);
}

export async function readNeighborProjection(signal?: AbortSignal): Promise<NeighborProjection> {
  return webJson<NeighborProjection>("GET", "/api/neighbors/sidebar", undefined, { signal });
}

export function useNeighborProjection(peer: string, tabKey: string): NeighborProjection {
  const [snapshot, setSnapshot] = useState<{ peer: string; projection: NeighborProjection } | null>(null);
  useEffect(() => {
    let live = true;
    let controller: AbortController | undefined;
    const stops: Array<() => void> = [];
    const load = () => {
      controller?.abort();
      const request = new AbortController();
      controller = request;
      void readNeighborProjection(request.signal).then(projection => {
        if (live && !request.signal.aborted) setSnapshot(current => ({ peer,
          projection: observeNeighborProjection(projection, current?.peer === peer ? current.projection : {}) }));
      }).catch(() => {
        if (live && !request.signal.aborted) setSnapshot(current => current?.peer === peer
          ? { peer, projection: observeNeighborProjection(current.projection, current.projection, true) } : current);
      });
    };
    const keep = (stop: () => void) => { if (live) stops.push(stop); else stop(); };
    for (const event of ["neighbor-agent-changed", "neighbor-activation-status", "neighbor-friend-paused", "neighbors-changed", "account-changed", "account-mesh-changed", "mesh-state-changed"]) void listen(event, load).then(keep);
    void listen<unknown>("neighbor-runtime-status", event => {
      const value = event.payload;
      if (receiverState(value) || (typeof value === "object" && value !== null && "receiver_unavailable" in value)) load();
    }).then(keep);
    void listen("neighbor-concierge-status", load).then(keep);
    stops.push(onResync(load));
    load();
    return () => { live = false; controller?.abort(); stops.forEach(stop => stop()); };
  }, [peer, tabKey]);
  return snapshot ? projectionForPeer(peer, snapshot.peer, snapshot.projection) : {};
}
