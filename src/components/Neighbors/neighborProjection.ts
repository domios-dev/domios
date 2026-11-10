import { useEffect, useState, useSyncExternalStore } from "react";
import { relayTarget, webJson } from "../../lib/webApi";
import { publishNeighborGateways } from "./neighborGateways";
import { resetNeighborStatus } from "./neighborStatusStore";
import { resetPausedFriends } from "./neighborPause";
import { resetNeighborReads } from "./neighborReads";
import { listen } from "../../lib/bridge";
import { onResync } from "../../lib/resync";

export type NeighborProjection = Record<string, {
  tab_id: string;
  friend_display_name: string;
  friend_photo_url: string | null;
  paused: boolean;
  access: "paid" | "locked" | null;
}>;

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
        if (live && !request.signal.aborted) setSnapshot({ peer, projection });
      }).catch(() => {});
    };
    const keep = (stop: () => void) => { if (live) stops.push(stop); else stop(); };
    for (const event of ["neighbor-agent-changed", "neighbors-changed", "account-changed"]) void listen(event, load).then(keep);
    stops.push(onResync(load));
    load();
    return () => { live = false; controller?.abort(); stops.forEach(stop => stop()); };
  }, [peer, tabKey]);
  return snapshot ? projectionForPeer(peer, snapshot.peer, snapshot.projection) : {};
}
