import { useEffect, type RefObject } from "react";
import { invoke, listen } from "../../lib/bridge";
import { updateRemoteChatPeers } from "../../lib/remoteChatStatus";
import type { MeshPeersResponse } from "../../lib/types";
import type { AccountMeshSnapshot } from "../../components/MeshPanel/accountMeshClient";
import { rememberAccountMachineNames, rememberMachineIdentities } from "../../lib/useMeshMachineIdentities";

export interface PairedRoster {
  peers: MeshPeersResponse;
  revision: number;
}

export function useMeshPeerProjection(
  remoteProjectionActiveRef: RefObject<boolean>,
  hasPairedPeersRef: RefObject<boolean | null>,
  markRemoteChannelsReconnecting: () => void,
  requestRemoteRefresh: () => void,
  acceptPairedPeers: (roster: PairedRoster) => void,
  settleInitialRoster: () => void,
) {
  useEffect(() => {
    remoteProjectionActiveRef.current = true;
    let active = true;
    let peerReadGeneration = 0;
    let identityReadGeneration = 0;
    let meshUnlisten: (() => void) | undefined;
    let identityUnlisten: (() => void) | undefined;
    const accountUnlisteners: Array<() => void> = [];
    let accountReadGeneration = 0;
    const refreshAccountNames = async () => {
      const generation = ++accountReadGeneration;
      try {
        const account = await invoke<AccountMeshSnapshot>("account_mesh_state");
        if (active && account && generation === accountReadGeneration) rememberAccountMachineNames(account);
      } catch {}
    };
    for (const event of ["account-changed", "account-mesh-changed", "mesh-state-changed"]) {
      void listen(event, refreshAccountNames).then(stop => {
        if (active) {
          accountUnlisteners.push(stop);
          if (event === "account-mesh-changed") void refreshAccountNames();
        } else stop();
      }).catch(() => {
        if (active && event === "account-mesh-changed") void refreshAccountNames();
      });
    }
    let muxUnlisten: (() => void) | undefined;
    const refreshPeers = async (identityOnly = false) => {
      const identityGeneration = ++identityReadGeneration;
      const generation = identityOnly ? peerReadGeneration : ++peerReadGeneration;
      if (!identityOnly) hasPairedPeersRef.current = null;
      try {
        const peers = await invoke<MeshPeersResponse>("mesh_list_peers");
        if (active && generation === peerReadGeneration && Array.isArray(peers?.paired)) {
          if (identityGeneration === identityReadGeneration) rememberMachineIdentities(peers);
          if (!identityOnly) {
            hasPairedPeersRef.current = peers.paired.length > 0;
            updateRemoteChatPeers(peers);
            acceptPairedPeers({ peers, revision: generation });
          }
        }
      } catch {}
      if (!identityOnly && active && generation === peerReadGeneration) {
        settleInitialRoster();
        void requestRemoteRefresh();
      }
    };
    const onMeshStateChanged = () => {
      markRemoteChannelsReconnecting();
      void refreshPeers();
    };
    void listen("mesh-state-changed", onMeshStateChanged).then((fn) => {
      if (active) {
        meshUnlisten = fn;
        void refreshPeers();
      } else fn();
    }).catch(() => { if (active) void refreshPeers(); });
    void listen("mesh-identity-changed", () => { void refreshPeers(true); }).then((fn) => {
      if (active) identityUnlisten = fn;
      else fn();
    });
    void listen("remote-workers-changed", () => {
      void requestRemoteRefresh();
    }).then((fn) => {
      if (active) muxUnlisten = fn;
      else fn();
    });
    return () => {
      active = false;
      remoteProjectionActiveRef.current = false;
      meshUnlisten?.();
      identityUnlisten?.();
      muxUnlisten?.();
      accountUnlisteners.forEach(stop => stop());
    };
  }, [remoteProjectionActiveRef, hasPairedPeersRef, markRemoteChannelsReconnecting, requestRemoteRefresh, acceptPairedPeers, settleInitialRoster]);
}
