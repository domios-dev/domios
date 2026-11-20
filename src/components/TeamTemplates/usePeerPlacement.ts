import { useAccountMachineNames } from "../../lib/useMeshMachineIdentities";
import { useCallback, useEffect, useState } from "react";
import { listen } from "../../lib/bridge";
import {
  createPeerWorkspace, hostListingFailure, listHostPeers, listPeerWorkspaces, placementMachinesFromHostPeers,
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
  const names = useAccountMachineNames();
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
    peers: listing?.peers.map(peer => ({ ...peer, name: names.peers.get(peer.id) ?? peer.name })) ?? null,
    peersReason: listing?.reason ?? null,
    machines: listing ? (listing.machines ?? placementMachinesFromHostPeers(listing.peers)).map(machine => ({
      ...machine, name: (machine.local ? names.local : names.peers.get(machine.id)) ?? machine.name,
    })) : null,
    peer: peer && { ...peer, name: names.peers.get(peer.id) ?? peer.name }, setPeer,
    workspaces, workspacesReason,
    workspace, setWorkspace,
    newName, setNewName,
    workspaceChosen,
    resolveWorkspace,
  };
}

export type PeerPlacement = ReturnType<typeof usePeerPlacement>;
