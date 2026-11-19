import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { MeshPeersResponse, MultiplexerState } from "../../lib/types";
import { setWorkspaceBrowseTarget, useWorkspaceBrowseTarget, workspaceBrowseRevision, workspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";
import type { PairedRoster } from "./useMeshPeerProjection";
import {
  pairedAvailability, projectBrowsedWorkspaces, projectPairedGeneral,
  type BrowsedWorkspace, type GeneralAvailability, type PairedGeneral,
} from "./pairedWorkspaceProjection";

interface GeneralSnapshot {
  sessions: PairedGeneral["sessions"];
  availability: "online" | "unreachable";
}

interface PeerRead {
  controller: AbortController;
  again: boolean;
}

interface WorkspaceBrowse {
  target: NonNullable<ReturnType<typeof workspaceBrowseTarget>>;
  availability: GeneralAvailability;
  workspaces: BrowsedWorkspace[];
}

export function usePairedWorkspaces(roster: PairedRoster | null) {
  const peers = roster?.peers ?? null;
  const rosterRevision = roster?.revision;
  const peerRoster = useRef<MeshPeersResponse | null>(peers);
  peerRoster.current = peers;
  const [general, setGeneral] = useState<ReadonlyMap<string, GeneralSnapshot>>(() => new Map());
  const reads = useRef(new Map<string, PeerRead>());

  const refreshGeneral = useCallback((peerId: string) => {
    const roster = peerRoster.current;
    const peer = roster?.paired.find(item => item.instance_id === peerId);
    if (!peer || pairedAvailability(peer, roster?.installations) !== "reconnecting") return;
    const running = reads.current.get(peerId);
    if (running) {
      running.again = true;
      return;
    }
    const read: PeerRead = { controller: new AbortController(), again: false };
    const { signal } = read.controller;
    reads.current.set(peerId, read);
    const publish = (next: (previous: GeneralSnapshot | undefined) => GeneralSnapshot) => {
      const currentRoster = peerRoster.current;
      const current = currentRoster?.paired.find(item => item.instance_id === peerId);
      if (!signal.aborted && current && current.public_key === peer.public_key
        && pairedAvailability(current, currentRoster?.installations) === "reconnecting") {
        setGeneral(previous => new Map(previous).set(peerId, next(previous.get(peerId))));
      }
    };
    void invoke<MultiplexerState>("mesh_peer_mux_state", { peerId, signal })
      .then(snapshot => {
        if (!signal.aborted) {
          const sessions = projectPairedGeneral(peerId, snapshot);
          publish(() => ({ sessions, availability: "online" }));
        }
      })
      .catch(() => publish(previous => ({ sessions: previous?.sessions ?? [], availability: "unreachable" })))
      .finally(() => {
        if (reads.current.get(peerId) !== read) return;
        reads.current.delete(peerId);
        if (read.again) refreshGeneral(peerId);
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
      if (peer && pairedAvailability(peer, peers.installations) === "reconnecting") continue;
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
    for (const id of paired) refreshGeneral(id);
  }, [rosterRevision, refreshGeneral]);

  const target = useWorkspaceBrowseTarget();
  const browseRevision = workspaceBrowseRevision();
  const selected = peers?.paired.find(peer => peer.instance_id === target?.id);
  const availability = selected ? pairedAvailability(selected, peers?.installations) : "reconnecting";
  const [response, setResponse] = useState<WorkspaceBrowse | null>(null);
  const requestBrowse = useRef<(() => void) | null>(null);
  const rosterReady = peers !== null;
  const selectedKey = selected?.public_key;
  const refreshBrowse = useCallback((peerId: string) => {
    if (workspaceBrowseTarget()?.id === peerId) requestBrowse.current?.();
  }, []);

  useEffect(() => {
    const target = workspaceBrowseTarget();
    if (!target || !rosterReady) return;
    if (selectedKey === undefined) {
      setWorkspaceBrowseTarget(null);
      return;
    }
    if (availability !== "reconnecting") return;
    const controller = new AbortController();
    const { signal } = controller;
    const current = () => !signal.aborted && workspaceBrowseRevision() === browseRevision;
    let running = false;
    let again = false;
    const read = async () => {
      if (!current()) return;
      if (running) {
        again = true;
        return;
      }
      running = true;
      try {
        do {
          again = false;
          try {
            const snapshot = await invoke<MultiplexerState>("mesh_peer_mux_state", { peerId: target.id, signal });
            if (current()) setResponse({ target, availability: "online", workspaces: projectBrowsedWorkspaces(target.id, snapshot) });
          } catch {
            if (current()) setResponse({ target, availability: "unreachable", workspaces: [] });
          }
        } while (again && current());
      } finally {
        running = false;
      }
    };
    requestBrowse.current = () => { void read(); };
    void read();
    return () => {
      controller.abort();
      requestBrowse.current = null;
    };
  }, [browseRevision, rosterReady, selectedKey, availability]);

  const browse = useMemo<WorkspaceBrowse | null>(() => {
    if (!target) return null;
    if (selected && availability === "reconnecting" && response?.target === target) return response;
    return { target, availability, workspaces: [] };
  }, [target, selected, response, availability]);

  const generals = useMemo<PairedGeneral[]>(() => (peers?.paired ?? []).map(peer => {
    const known = general.get(peer.instance_id);
    const observed = pairedAvailability(peer, peers?.installations);
    return {
      peerId: peer.instance_id,
      name: peer.instance_name,
      availability: observed === "reconnecting" ? known?.availability ?? observed : observed,
      sessions: known?.sessions ?? [],
    };
  }), [general, peers]);
  return { generals, browse, refreshGeneral, refreshBrowse };
}
