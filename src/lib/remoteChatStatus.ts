import { useSyncExternalStore } from "react";
import type { MeshPeersResponse } from "./types";
import { parseCompositeTabId } from "./remoteTabTransport";
import { remoteChatState, type RemoteChatFailure, type RemoteChatPresence } from "./remoteChatState";

interface Peer {
  name?: string;
  presence: RemoteChatPresence;
}

let loaded = false;
let peers = new Map<string, Peer>();
const failures = new Map<string, RemoteChatFailure>();
const listeners = new Set<() => void>();
let revision = 0;
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
};
const snapshot = () => revision;
const noSubscription = () => () => {};
function publish() {
  revision++;
  listeners.forEach(listener => listener());
}

export function updateRemoteChatPeers(response: MeshPeersResponse) {
  const next = new Map<string, Peer>();
  const put = (id: string, presence: RemoteChatPresence, name?: string) => {
    const prior = peers.get(id);
    next.set(id, { presence, name: name || prior?.name });
  };
  for (const peer of response.paired) put(peer.instance_id, peer.online ? "online" : "offline", peer.instance_name);
  for (const installation of response.installations ?? []) {
    const id = installation.instance_id ?? installation.installation_id;
    if (installation.signed_out && installation.active) put(id, "signed-out", installation.name);
    else if (!next.has(id) && installation.active) put(id, "offline", installation.name);
  }
  for (const [id, peer] of peers) {
    if (!next.has(id)) put(id, "not-paired", peer.name);
  }
  const changed = !loaded || next.size !== peers.size || [...next].some(([id, peer]) =>
    peer.presence !== peers.get(id)?.presence || peer.name !== peers.get(id)?.name);
  const previous = peers;
  peers = next;
  loaded = true;
  if (!changed) return;
  for (const [tabId] of failures) {
    const id = parseCompositeTabId(tabId)?.peerId;
    if (id && next.get(id)?.presence === "online" && previous.get(id)?.presence !== "online") failures.delete(tabId);
  }
  publish();
}

export function resetRemoteChatStatus() {
  peers.clear();
  failures.clear();
  loaded = false;
  publish();
}

export function remoteChatPeer(peerId?: string | null): Peer | undefined {
  if (!peerId || !loaded) return undefined;
  return peers.get(peerId) ?? { presence: "not-paired" };
}

export function setRemoteChatFailure(tabId: string, failure?: RemoteChatFailure) {
  if (failure) failures.set(tabId, failure);
  else if (!failures.delete(tabId)) return;
  publish();
}

export function clearPeerRemoteChatFailures(peerId: string) {
  let deleted = false;
  for (const tabId of [...failures.keys()]) {
    if (parseCompositeTabId(tabId)?.peerId === peerId) {
      failures.delete(tabId);
      deleted = true;
    }
  }
  if (deleted) publish();
}

export function readRemoteChatStatus(tabId: string, peerId?: string | null) {
  const peer = remoteChatPeer(peerId);
  const failure = peerId ? failures.get(tabId) : undefined;
  return { peer, failure, state: peerId ? remoteChatState(peer?.presence, failure) : null };
}

export function useRemoteChatStatus(tabId: string, peerId?: string | null) {
  useSyncExternalStore(peerId ? subscribe : noSubscription, snapshot, snapshot);
  return readRemoteChatStatus(tabId, peerId);
}
