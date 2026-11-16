import { useCallback, useEffect, useState } from "react";
import { listen } from "../../lib/bridge";
import {
  createPeerWorkspace, hostListingFailure, listHostPeers, listPeerWorkspaces,
  type HostPeer, type HostPeerListing, type PeerWorkspace,
} from "../../lib/teamTemplates/hostPeers";

/** The workspace choice that creates a fresh provisioned workspace on the peer. */
export const NEW_PEER_WORKSPACE = "__new__";

/**
 * Where the whole Team starts: this machine, or a paired peer and one of its own workspaces.
 * The peer list loads when wanted and follows every mesh change while the form is open.
 */
export function usePeerPlacement({ wanted, listPeers = listHostPeers, listWorkspaces = listPeerWorkspaces, createWorkspace = createPeerWorkspace }: {
  wanted: boolean;
  listPeers?: () => Promise<HostPeerListing>;
  listWorkspaces?: (peerId: string) => Promise<PeerWorkspace[]>;
  createWorkspace?: (peerId: string, name: string) => Promise<string>;
}) {
  const [listing, setListing] = useState<HostPeerListing | null>(null);
  const [peer, setPeerState] = useState<HostPeer | null>(null);
  const [workspaces, setWorkspaces] = useState<PeerWorkspace[] | null>(null);
  const [workspacesReason, setWorkspacesReason] = useState<string | null>(null);
  const [workspace, setWorkspace] = useState("");
  const [newName, setNewName] = useState("");

  useEffect(() => {
    if (!wanted) return;
    let live = true;
    let stop: (() => void) | undefined;
    const load = () => {
      void listPeers().then(
        (next) => { if (live) setListing(next); },
        (error: unknown) => { if (live) setListing({ peers: [], reason: hostListingFailure(error) }); },
      );
    };
    load();
    void listen("mesh-state-changed", load).then((unlisten) => {
      if (live) stop = unlisten;
      else unlisten();
    }, () => undefined);
    return () => { live = false; stop?.(); };
  }, [wanted, listPeers]);

  // A machine that went offline or lost Full access leaves the choice.
  useEffect(() => {
    if (peer && listing && !listing.peers.some((candidate) => candidate.id === peer.id)) setPeerState(null);
  }, [peer, listing]);

  useEffect(() => {
    setWorkspaces(null);
    setWorkspacesReason(null);
    if (!peer) return;
    let live = true;
    void listWorkspaces(peer.id).then(
      (next) => { if (live) setWorkspaces(next); },
      (error: unknown) => {
        if (!live) return;
        setWorkspaces([]);
        setWorkspacesReason(`${peer.name} did not list its workspaces: ${error instanceof Error ? error.message : String(error)}`);
      },
    );
    return () => { live = false; };
  }, [peer, listWorkspaces]);

  const setPeer = useCallback((peerId: string | null) => {
    setPeerState(peerId ? listing?.peers.find((candidate) => candidate.id === peerId) ?? null : null);
    setWorkspace("");
  }, [listing]);

  const workspaceChosen = workspace === NEW_PEER_WORKSPACE ? newName.trim().length > 0 : workspace !== "";

  /** The peer workspace the Team starts in; a new one is created on the peer first. */
  const resolveWorkspace = useCallback(async (): Promise<string> => {
    if (!peer || !workspaceChosen) throw new Error("Choose a workspace on the other machine first");
    return workspace === NEW_PEER_WORKSPACE ? createWorkspace(peer.id, newName.trim()) : workspace;
  }, [peer, workspace, workspaceChosen, newName, createWorkspace]);

  return {
    peers: listing?.peers ?? null,
    peersReason: listing?.reason ?? null,
    peer, setPeer,
    workspaces, workspacesReason,
    workspace, setWorkspace,
    newName, setNewName,
    workspaceChosen,
    resolveWorkspace,
  };
}

export type PeerPlacement = ReturnType<typeof usePeerPlacement>;
