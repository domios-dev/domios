import { useEffect, useRef } from "react";
import { peerEventRail } from "../../lib/peerEventRail";
import { pairedAvailability } from "./pairedWorkspaceProjection";
import type { PairedRoster } from "./useMeshPeerProjection";

export function usePeerStatusRail(roster: PairedRoster | null, wake: (peerId: string) => void) {
  const subscriptions = useRef(new Map<string, { link: string; stop: () => void }>());
  useEffect(() => {
    const active = subscriptions.current;
    const peers = new Map((roster?.peers.paired ?? [])
      .filter(peer => pairedAvailability(peer, roster?.peers.installations) === "reconnecting")
      .map(peer => [peer.instance_id, JSON.stringify([peer.public_key, peer.paired_at])]));
    for (const [peerId, current] of active) {
      if (current.link === peers.get(peerId)) continue;
      current.stop();
      active.delete(peerId);
    }
    for (const [peerId, link] of peers) {
      if (active.has(peerId)) peerEventRail.resume(peerId);
      else active.set(peerId, { link, stop: peerEventRail.subscribe(peerId, link, () => wake(peerId)) });
    }
  }, [roster, wake]);
  useEffect(() => {
    const active = subscriptions.current;
    const gateway = (event: Event) => {
      if ((event as CustomEvent<{ available?: boolean }>).detail?.available) {
        for (const peerId of active.keys()) peerEventRail.resume(peerId);
      }
    };
    window.addEventListener("codeterm-gateway-state", gateway);
    return () => {
      window.removeEventListener("codeterm-gateway-state", gateway);
      for (const { stop } of active.values()) stop();
      active.clear();
    };
  }, []);
}
