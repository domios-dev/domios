import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { cleanMachineName } from "../../lib/machineIdentity";
import { invoke } from "../../lib/bridge";
import type { MeshPeersResponse, MultiplexerState } from "../../lib/types";
import { setWorkspaceBrowseTarget, useWorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";
import { pairedGeneralCacheScope, readPairedGeneralCache, writePairedGeneralCache, type CachedPeerGeneral } from "./pairedGeneralCache";
import type { PairedRoster } from "./useMeshPeerProjection";
import {
  pairedAvailability, projectBrowsedWorkspaces, projectPairedGeneral,
  type BrowsedWorkspace, type GeneralAvailability, type PairedGeneral,
} from "./pairedWorkspaceProjection";

type CacheAccount = string | false | null;

interface GeneralSnapshot {
  account: CacheAccount;
  publicKey: string;
  sessions?: PairedGeneral["sessions"];
  teamNames?: PairedGeneral["teamNames"];
  workspaces?: BrowsedWorkspace[];
  availability: "online" | "unreachable";
}

interface PeerRead {
  account: CacheAccount;
  controller: AbortController;
  again: boolean;
}

interface WorkspaceBrowse {
  target: NonNullable<ReturnType<typeof workspaceBrowseTarget>>;
  availability: GeneralAvailability;
  workspaces: BrowsedWorkspace[];
}

export function usePairedWorkspaces(roster: PairedRoster | null, rosterSettled: boolean, cacheAccount: CacheAccount) {
  const peers = roster?.peers ?? null;
  const rosterRevision = roster?.revision;
  const peerRoster = useRef<MeshPeersResponse | null>(peers);
  peerRoster.current = peers;
  const accountRef = useRef(cacheAccount);
  accountRef.current = cacheAccount;
  const [general, setGeneral] = useState<ReadonlyMap<string, GeneralSnapshot>>(() => new Map());
  const reads = useRef(new Map<string, PeerRead>());
  const cacheScope = cacheAccount ? pairedGeneralCacheScope(cacheAccount) : null;
  const [cached, setCached] = useState<{ scope: string; peers: ReadonlyMap<string, CachedPeerGeneral> }>();
  const cachedPeers = cached?.scope === cacheScope ? cached?.peers : undefined;
  const ready = cacheAccount !== false
    && (!cacheScope || cachedPeers !== undefined)
    && (!cachedPeers?.size || rosterSettled);
  useEffect(() => {
    if (!cacheScope) return;
    let active = true;
    void readPairedGeneralCache(cacheScope).then(peers => {
      if (active) setCached({ scope: cacheScope, peers });
    });
    return () => { active = false; };
  }, [cacheScope]);

  useEffect(() => {
    if (!cacheScope || !cachedPeers || !peers) return;
    const next = new Map<string, CachedPeerGeneral>();
    for (const peer of peers.paired) {
      const fresh = general.get(peer.instance_id);
      const saved = cachedPeers.get(peer.instance_id);
      if (fresh?.account === cacheAccount && fresh.publicKey === peer.public_key && fresh.sessions) {
        if (fresh.sessions.length) next.set(peer.instance_id, { publicKey: peer.public_key, sessions: fresh.sessions });
      } else if (saved?.publicKey === peer.public_key) {
        next.set(peer.instance_id, saved);
      }
    }
    void writePairedGeneralCache(cacheScope, next);
  }, [cacheAccount, cacheScope, cachedPeers, general, peers]);

  const refreshPeerInventory = useCallback((peerId: string) => {
    const roster = peerRoster.current;
    const peer = roster?.paired.find(item => item.instance_id === peerId);
    if (!peer || pairedAvailability(peer, roster?.installations) !== "reconnecting") return;
    const running = reads.current.get(peerId);
    if (running) {
      running.again = true;
      return;
    }
    const account = accountRef.current;
    const read: PeerRead = { account, controller: new AbortController(), again: false };
    const { signal } = read.controller;
    reads.current.set(peerId, read);
    const publish = (next: (previous: GeneralSnapshot | undefined) => GeneralSnapshot) => {
      const currentRoster = peerRoster.current;
      const current = currentRoster?.paired.find(item => item.instance_id === peerId);
      if (!signal.aborted && accountRef.current === account && current && current.public_key === peer.public_key
        && pairedAvailability(current, currentRoster?.installations) === "reconnecting") {
        setGeneral(previous => {
          const known = previous.get(peerId);
          return new Map(previous).set(peerId, next(known?.account === account && known.publicKey === peer.public_key ? known : undefined));
        });
      }
    };
    void invoke<MultiplexerState>("mesh_peer_mux_state", { peerId, signal })
      .then(snapshot => {
        if (!signal.aborted) {
          const sessions = projectPairedGeneral(peerId, snapshot);
          const teamNames = new Map((snapshot.teams ?? []).flatMap(team => {
            const name = team.name?.trim();
            return name && name !== team.id ? [[team.id, name] as const] : [];
          }));
          const workspaces = projectBrowsedWorkspaces(peerId, snapshot);
          publish(() => ({ account, publicKey: peer.public_key, sessions, teamNames, workspaces, availability: "online" }));
        }
      })
      .catch(() => publish(previous => ({ account, publicKey: peer.public_key, sessions: previous?.sessions, teamNames: previous?.teamNames, availability: "unreachable" })))
      .finally(() => {
        if (reads.current.get(peerId) !== read) return;
        reads.current.delete(peerId);
        if (read.again) refreshPeerInventory(peerId);
      });
  }, []);

  useEffect(() => {
    const running = reads.current;
    return () => {
      for (const read of running.values()) read.controller.abort();
      running.clear();
    };
  }, []);

  useEffect(() => {
    const peers = peerRoster.current;
    if (!peers) return;
    const paired = new Set(peers.paired.map(peer => peer.instance_id));
    for (const [peerId, read] of reads.current) {
      const peer = peers.paired.find(item => item.instance_id === peerId);
      if (read.account === cacheAccount && peer && pairedAvailability(peer, peers.installations) === "reconnecting") continue;
      reads.current.delete(peerId);
      read.controller.abort();
    }
    setGeneral(previous => {
      const missing = [...previous.keys()].filter(id => !paired.has(id));
      if (!missing.length) return previous;
      const next = new Map(previous);
      for (const id of missing) next.delete(id);
      return next;
    });
    for (const id of paired) refreshPeerInventory(id);
  }, [rosterRevision, refreshPeerInventory, cacheAccount]);

  const target = useWorkspaceBrowseTarget();
  const targetId = target?.id;
  const selected = peers?.paired.find(peer => peer.instance_id === targetId);
  const rosterReady = peers !== null;
  const selectedKey = selected?.public_key;

  useEffect(() => {
    if (!targetId || !rosterReady) return;
    if (selectedKey === undefined) {
      setWorkspaceBrowseTarget(null);
      return;
    }
    refreshPeerInventory(targetId);
  }, [targetId, rosterReady, selectedKey, refreshPeerInventory]);

  const browse = useMemo<WorkspaceBrowse | null>(() => {
    if (!target) return null;
    const availability = selected ? pairedAvailability(selected, peers?.installations) : "reconnecting";
    const snapshot = general.get(target.id);
    if (availability === "reconnecting" && selected && snapshot?.account === cacheAccount && snapshot.publicKey === selected.public_key) {
      return { target, availability: snapshot.availability, workspaces: snapshot.workspaces ?? [] };
    }
    return { target, availability, workspaces: [] };
  }, [target, selected, peers?.installations, general, cacheAccount]);

  const generals = useMemo<PairedGeneral[]>(() => (peers?.paired ?? []).map(peer => {
    const live = general.get(peer.instance_id);
    const saved = cachedPeers?.get(peer.instance_id);
    const known = live?.account === cacheAccount && live.publicKey === peer.public_key ? live : undefined;
    const observed = pairedAvailability(peer, peers?.installations);
    return {
      peerId: peer.instance_id,
      name: cleanMachineName(peer.instance_name, peer.instance_id) ?? "Machine",
      teamNames: known?.teamNames,
      availability: observed === "reconnecting" ? known?.availability ?? observed : observed,
      sessions: known?.sessions ?? (saved?.publicKey === peer.public_key ? saved.sessions : []),
      cached: !known?.sessions && saved !== undefined && saved.publicKey === peer.public_key,
    };
  }), [general, peers, cachedPeers, cacheAccount]);
  return { generals, browse, refreshPeerInventory, ready };
}
